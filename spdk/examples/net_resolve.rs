use itertools::Itertools;

use spdk::{main, net::resolve};

const IVP4_UNSPECIFIED: &str = "0.0.0.0";
const IPV4_LOCALHOST: &str = "127.0.0.1";
const IPV6_LOCALHOST: &str = "::0";
const LOCALHOST: &str = "localhost";
const RUST_LANG: &str = "www.rust-lang.org";

const SERVICE_PORT_NUM: &str = "8080";
const SERVICE_PORT_FTP: &str = "ftp";
const SERVICE_PORT_HTTPS: &str = "https";

#[main]
async fn main() {
    let addr = resolve((IVP4_UNSPECIFIED, Some(SERVICE_PORT_NUM)))
        .await
        .unwrap()
        .join(", ");

    println!("{}:{} => {}", IVP4_UNSPECIFIED, SERVICE_PORT_NUM, addr);

    let addr = resolve((IPV4_LOCALHOST, Some(SERVICE_PORT_NUM)))
        .await
        .unwrap()
        .join(", ");

    println!("{}:{} => {}", IPV4_LOCALHOST, SERVICE_PORT_NUM, addr);

    let addr = resolve((IPV4_LOCALHOST, None)).await.unwrap().join(", ");

    println!("{} => {}", IPV4_LOCALHOST, addr);

    let addr = resolve((IPV6_LOCALHOST, Some(SERVICE_PORT_NUM)))
        .await
        .unwrap()
        .join(", ");

    println!("[{}]:{} => {}", IPV6_LOCALHOST, SERVICE_PORT_NUM, addr);

    let addr = resolve((IPV6_LOCALHOST, None)).await.unwrap().join(", ");

    println!("[{}] => {}", IPV6_LOCALHOST, addr);

    let addr = resolve((LOCALHOST, Some(SERVICE_PORT_FTP)))
        .await
        .unwrap()
        .join(", ");

    println!("{}:{} => {}", LOCALHOST, SERVICE_PORT_FTP, addr);

    let addr = resolve((RUST_LANG, Some(SERVICE_PORT_HTTPS)))
        .await
        .unwrap()
        .join(", ");

    println!("{}:{} => {}", RUST_LANG, SERVICE_PORT_HTTPS, addr);

    let addr = resolve(format!("{}:{}", IPV4_LOCALHOST, SERVICE_PORT_NUM))
        .await
        .unwrap()
        .join(", ");

    println!("{}:{} => {}", IPV4_LOCALHOST, SERVICE_PORT_NUM, addr);

    let addr = resolve(format!("[{}]:{}", IPV6_LOCALHOST, SERVICE_PORT_NUM))
        .await
        .unwrap()
        .join(", ");

    println!("[{}]:{} => {}", IPV6_LOCALHOST, SERVICE_PORT_NUM, addr);

    let addr = resolve(format!("{}:{}", RUST_LANG, SERVICE_PORT_NUM))
        .await
        .unwrap()
        .join(", ");

    println!("{}:{} => {}", RUST_LANG, SERVICE_PORT_NUM, addr);
}
