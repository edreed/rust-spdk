use futures::{AsyncReadExt, AsyncWriteExt};

use spdk::{
    self,
    cli::Parser,
    errors,
    net::{TcpListener, TcpStream},
    thread,
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

#[spdk::main(cli_args = Args::parse())]
async fn main() {
    let args = Args::get();

    let join: spdk::task::JoinHandle<Result<(), errors::Errno>> = if args.server_mode {
        thread::spawn_local(async move {
            let mut listener = TcpListener::bind(args.host.as_str()).await?;
            let mut client = listener.accept().await?;

            let mut msg = String::new();

            client.read_to_string(&mut msg).await?;

            println!(
                "SERVER: Read \"{}\" from client {}",
                msg,
                client.peer_addr().expect("connected")
            );

            Ok(())
        })
    } else {
        thread::spawn_local(async move {
            println!("CLIENT: Connecting to {}", args.host);

            let mut server = TcpStream::connect(args.host.as_str()).await?;

            println!("CLIENT: Writing \"{}\" to server", HELLO_WORLD);

            server.write_all(HELLO_WORLD.as_bytes()).await?;

            Ok(())
        })
    };

    join.await.expect("success");
}
