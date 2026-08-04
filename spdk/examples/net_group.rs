use std::ffi::CString;

use futures::{AsyncReadExt, AsyncWriteExt};

use spdk::{
    self,
    cli::Parser,
    net::{Accepted, SocketGroup, TcpSocketExt, TcpSocketRemote, TcpStream},
    task::JoinHandle,
    thread::{self, Thread},
};

#[derive(Debug, Parser)]
struct Args {
    /// Whether to start the application in server mode.
    server_mode: bool,

    /// The host address and port.
    #[spdk_arg(default = "localhost:8080".into())]
    host: String,
}

const HELLO_WORLD: &str = "Hello, World!";

async fn handle_client(client: Accepted) -> spdk::Result<()> {
    let name = CString::new(format!(
        "client {}",
        client.peer_addr().expect("client connected")
    ))
    .unwrap();

    Thread::new(&name, &Thread::current().cpuset())?
        .spawn(move || async {
            let group = SocketGroup::new();
            let mut client = group.add(client)?;
            let mut msg = String::new();

            client.read_to_string(&mut msg).await?;

            println!(
                "SERVER: Read \"{}\" from client {}",
                msg,
                client.peer_addr().expect("connected")
            );

            Ok(())
        })
        .await
}

fn run_server(args: &'static Args) -> JoinHandle<spdk::Result<()>> {
    thread::spawn_local(async move {
        let group = SocketGroup::new();
        let mut listener = group.bind(args.host.as_str()).await?;

        println!(
            "SERVER: Listening on {}",
            listener.local_addr().expect("listener bound")
        );

        let client = listener.accept().await?;

        handle_client(client).await
    })
}

fn run_client(args: &'static Args) -> JoinHandle<spdk::Result<()>> {
    thread::spawn_local(async move {
        println!("CLIENT: Connecting to {}", args.host);

        let mut server = TcpStream::connect(args.host.as_str()).await?;

        println!("CLIENT: Writing \"{}\" to server", HELLO_WORLD);

        server.write_all(HELLO_WORLD.as_bytes()).await?;

        Ok(())
    })
}

#[spdk::main(cli_args = Args::parse())]
async fn main() {
    let args = Args::get();

    if args.server_mode {
        run_server(args).await.expect("server succeeeded");
    } else {
        run_client(args).await.expect("client succeeded")
    };
}
