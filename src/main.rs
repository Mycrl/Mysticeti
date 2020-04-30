extern crate bytes;
extern crate rml_rtmp;
extern crate tokio;

mod rtmp;
mod server;

use futures::executor::block_on;
use std::error::Error;
use server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr = "0.0.0.0:1935".parse().unwrap();
    block_on(Server::new(addr).await?)?;
    Ok(())
}
