use anyhow::Result;
use bytes::BytesMut;
use super::{
    Context, 
    Response
};

use stun::{
    Kind, 
    MessageReader,
    MessageWriter
};

use stun::attribute::{
    ErrKind,
    Error,
    ErrorCode,
    Realm,
    UserName,
    ChannelNumber,
    XorPeerAddress
};

use stun::attribute::ErrKind::{
    BadRequest,
    Unauthorized,
    AllocationMismatch,
};

/// return channel binding error response
#[inline(always)]
fn reject<'a>(
    ctx: Context, 
    m: MessageReader<'a>, 
    w: &'a mut BytesMut,
    e: ErrKind, 
) -> Result<Response<'a>> {
    let mut pack = MessageWriter::derive(Kind::CreatePermissionError, &m, w);
    pack.append::<ErrorCode>(Error::from(e));
    pack.append::<Realm>(&ctx.conf.realm);
    pack.try_into(None)?;
    Ok(Some((w, ctx.addr)))
}

/// return channel binding ok response
#[inline(always)]
fn resolve<'a>(
    ctx: &Context, 
    m: &MessageReader, 
    u: &str, 
    p: &str, 
    w: &'a mut BytesMut
) -> Result<Response<'a>> {
    MessageWriter::derive(Kind::ChannelBindResponse, m, w)
        .try_into(Some((u, p, &ctx.conf.realm)))?;
    Ok(Some((w, ctx.addr.clone())))
}

/// process channel binding request
///
/// The server MAY impose restrictions on the IP address and port values
/// allowed in the XOR-PEER-ADDRESS attribute; if a value is not allowed,
/// the server rejects the request with a 403 (Forbidden) error.
///
/// If the request is valid, but the server is unable to fulfill the
/// request due to some capacity limit or similar, the server replies
/// with a 508 (Insufficient Capacity) error.
///
/// Otherwise, the server replies with a ChannelBind success response.
/// There are no required attributes in a successful ChannelBind
/// response.
///
/// If the server can satisfy the request, then the server creates or
/// refreshes the channel binding using the channel number in the
/// CHANNEL-NUMBER attribute and the transport address in the XOR-PEER-
/// ADDRESS attribute.  The server also installs or refreshes a
/// permission for the IP address in the XOR-PEER-ADDRESS attribute as
/// described in Section 9.
///
/// NOTE: A server need not do anything special to implement
/// idempotency of ChannelBind requests over UDP using the
/// "stateless stack approach".  Retransmitted ChannelBind requests
/// will simply refresh the channel binding and the corresponding
/// permission.  Furthermore, the client must wait 5 minutes before
/// binding a previously bound channel number or peer address to a
/// different channel, eliminating the possibility that the
/// transaction would initially fail but succeed on a
/// retransmission.
#[rustfmt::skip]
pub async fn process<'a>(ctx: Context, m: MessageReader<'a>, w: &'a mut BytesMut) -> Result<Response<'a>> {
    let u = match m.get::<UserName>() {
        Some(u) => u?,
        _ => return reject(ctx, m, w, Unauthorized),
    };

    let c = match m.get::<ChannelNumber>() {
        Some(c) => c?,
        _ => return reject(ctx, m, w, BadRequest),
    };
    
    let p = match m.get::<XorPeerAddress>() {
        Some(a) => a?.port(),
        _ => return reject(ctx, m, w, BadRequest)
    };

    if !(0x4000..=0x4FFF).contains(&c) {
        return reject(ctx, m, w, BadRequest)
    }

    let key = match ctx.state.get_password(&ctx.addr, u).await {
        None => return reject(ctx, m, w, Unauthorized),
        Some(a) => a,
    };

    if m.integrity((u, &key, &ctx.conf.realm)).is_err() {
        return reject(ctx, m, w, Unauthorized);
    }
    
    if !ctx.state.insert_channel(ctx.addr.clone(), p, c).await {
        return reject(ctx, m, w, AllocationMismatch);
    }
    
    log::info!(
        "{:?} [{:?}] bind channel={}", 
        &ctx.addr,
        u,
        c
    );

    resolve(&ctx, &m, u, &key, w)
}
