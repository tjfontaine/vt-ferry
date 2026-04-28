use vtf_host_worker::protocol::*;
use std::mem::size_of;

fn main() {
    println!("MessageHeader size: {}", size_of::<MessageHeader>());
    println!("HelloReply size: {}", size_of::<HelloReply>());
}
