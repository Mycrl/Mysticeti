use super::{util, AttrKind, Auth, ChannelData, Kind, Message, Property};
use std::convert::TryFrom;

use anyhow::{anyhow, ensure, Result};

use bytes::{BufMut, BytesMut};

const ZOER_BUF: [u8; 10] = [0u8; 10];
const UNKNOWN_PAYLOAD: Message = Message {
    kind: Kind::Unknown,
    attributes: vec![],
    buffer: &[],
    token: &[],
    block: 0,
};

/// 解码消息
///
/// 注意: 不支持的消息属性将忽略

pub fn decode_message<'a>(buffer: &'a [u8]) -> Result<Message<'a>> {
    ensure!(buffer.len() >= 20, "message len < 20");
    let count_size = buffer.len();
    let mut attributes = Vec::new();
    let mut find_block = false;
    let mut block = 0;

    // 消息类型
    let kind = Kind::try_from(util::as_u16(&buffer[..2])).unwrap_or(Kind::Unknown);

    // 当消息类型不受支持时
    // 直接返回未定义消息类型
    if Kind::Unknown == kind {
        return Ok(UNKNOWN_PAYLOAD);
    }

    // 消息长度
    // magic cookie
    let size = util::as_u16(&buffer[2..4]) as usize;
    let cookie = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

    // 检查固定Cookie
    // 检查长度是否足够
    ensure!(cookie == 0x2112A442, "missing cookie");
    ensure!(count_size >= size + 20, "missing len");

    // 获取交易号
    // 创建偏移量
    let token = &buffer[8..20];
    let mut offset = 20;

    loop {
        // 如果长度不够继续完成，
        // 则跳出循环返回所有的字段.
        if count_size - offset < 4 {
            break;
        }

        // 获取属性类型
        let key = u16::from_be_bytes([buffer[offset], buffer[offset + 1]]);

        // 是否已经找到消息一致性摘要
        // 如果已经找到则记录当前偏移位置
        if !find_block {
            block = offset as u16;
        }

        // 检查当前属性是否为消息一致性摘要
        // 如果是则标记已经找到此属性
        if key == AttrKind::MessageIntegrity as u16 {
            find_block = true;
        }

        // 获取属性长度
        let size = u16::from_be_bytes([buffer[offset + 2], buffer[offset + 3]]) as usize;

        // 检查剩余内容长度
        // 这里可以避免长度溢出
        offset += 4;
        if count_size - offset < size {
            break;
        }

        // 获取属性内容
        let psize = util::pad_size(size);
        let value = &buffer[offset..offset + size];

        // 此处为了兼容填充位，
        // 将跳过填充长度
        if size > 0 {
            offset += size + psize;
        }

        // 检查是否为受支持类型
        // 不受支持类型直接跳过
        let dyn_attribute = match AttrKind::try_from(key) {
            Ok(a) => a,
            Err(_) => continue,
        };

        // 如果是受支持的类型，
        // 则写入到属性列表
        if let Ok(attribute) = dyn_attribute.from(token, value) {
            attributes.push((dyn_attribute, attribute));
        }
    }

    Ok(Message {
        kind,
        block,
        token,
        buffer,
        attributes,
    })
}

/// 编码消息
///
/// 将消息结构编码为缓冲区.

pub fn encode_message(message: Message, buf: &mut BytesMut, auth: Option<Auth>) -> Result<()> {
    assert_ne!(message.kind, Kind::Unknown);
    unsafe { buf.set_len(0) }

    // 消息类型
    // 消息长度
    // 固定Cookie
    // 交易号
    buf.put_u16(message.kind as u16);
    buf.put_u16(0);
    buf.put_u32(0x2112A442);
    buf.put(message.token);

    // 属性列表
    for (k, v) in message.attributes {
        buf.put_u16(k as u16);

        // 记录当前位置
        // 然后推进内部游标2个字节
        // 这里的用意为预留出位置等待后续写入
        let os = buf.len();
        unsafe { buf.advance_mut(2) }
        v.into_bytes(buf, message.token);

        // 计算写入长度
        // 会到原始位置写入长度
        let size = buf.len() - os - 2;
        let size_buf = (size as u16).to_be_bytes();
        buf[os] = size_buf[0];
        buf[os + 1] = size_buf[1];

        // 如果需要填充
        // 则填充空值
        let psize = util::pad_size(size);
        if psize > 0 {
            buf.put(&ZOER_BUF[0..psize]);
        }
    }

    // 重新填充属性长度
    // 直接更改底层内存缓冲区
    let attr_size = (buf.len() - 20) as u16;
    let size_buf = attr_size.to_be_bytes();
    buf[2] = size_buf[0];
    buf[3] = size_buf[1];

    // 是否需要摘要
    if let Some(a) = auth {
        encoder_integrity(buf, a)?;
    }

    Ok(())
}

/// 消息完整性摘要编码
///
/// 使消息包含`消息完整性检查`属性，
/// 并将消息序列化为缓冲区

pub fn encoder_integrity(buffer: &mut BytesMut, auth: Auth) -> Result<()> {
    assert!(buffer.len() >= 20);

    // 计算新的消息长度
    // 新的长度包含MessageIntegrity字段长度
    let mut buffer_size = (buffer.len() + 4) as u16;
    let size_buf = buffer_size.to_be_bytes();

    // 将新的长度覆盖原有长度
    buffer[2] = size_buf[0];
    buffer[3] = size_buf[1];

    // 长期认证KEY
    // 对消息缓冲区进行摘要
    // 创建新的MessageIntegrity属性
    let key = util::long_key(auth.0, auth.1, auth.2);
    let hmac_output = util::hmac_sha1(&key, vec![&buffer])?.into_bytes();
    let property_buf = hmac_output.as_slice();

    // 消息一致性摘要属性
    buffer.put_u16(AttrKind::MessageIntegrity as u16);
    buffer.put_u16(20);
    buffer.put(property_buf);

    // 计算新的消息长度
    // 新的长度包含Fingerprint字段长度
    buffer_size += 8;
    let size_buf = buffer_size.to_be_bytes();

    // 将新的长度覆盖原有长度
    buffer[2] = size_buf[0];
    buffer[3] = size_buf[1];

    // CRC Fingerprint
    buffer.put_u16(AttrKind::Fingerprint as u16);
    buffer.put_u16(4);
    buffer.put_u32(util::fingerprint(&buffer));

    Ok(())
}

/// 消息完整性检查
///
/// 检查消息中包含的`消息完整性检查`属性
/// 返回是否认证一致

pub fn assert_integrity(payload: &Message<'_>, auth: Auth) -> Result<bool> {
    assert!(!payload.buffer.is_empty());
    assert!(payload.block > 20);

    // 展开原始消息属性
    // 如不存在则返回错误
    let integrity = payload
        .get(AttrKind::MessageIntegrity)
        .ok_or_else(|| anyhow!("not found MessageIntegrity"))?;

    // 构建多段提交
    // 单独提交新的长度
    let size_buf = (payload.block + 4).to_be_bytes();
    let body = vec![
        &payload.buffer[0..2],
        &size_buf,
        &payload.buffer[4..payload.block as usize],
    ];

    // 对消息属性整体摘要
    let key = util::long_key(auth.0, auth.1, auth.2);
    let hmac_output = util::hmac_sha1(&key, body)?.into_bytes();
    let property_buf = hmac_output.as_slice();

    // 检查摘要和原始摘要
    // 返回是否一致
    Ok(match integrity {
        Property::MessageIntegrity(x) => &property_buf == x,
        _ => false,
    })
}

/// 解码频道数据

pub fn decode_channel(buf: &[u8]) -> Result<ChannelData<'_>> {
    let len = buf.len();
    ensure!(len >= 4, "data len < 4");
    let size = util::as_u16(&buf[2..4]) as usize;
    ensure!(size <= len - 4, "data body len < size");
    Ok(ChannelData {
        number: util::as_u16(&buf[..2]),
        buf,
    })
}
