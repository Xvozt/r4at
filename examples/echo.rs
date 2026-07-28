use tokio::io::{self, AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> io::Result<()> {
    let addr = "127.0.0.1:6969";
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0; 10];
                let (mut r_stream, mut w_stream) = stream.into_split();
                let Ok(n) = r_stream.read(&mut buf[..]).await else {
                    return;
                };
                let _ = w_stream.write_all(&buf[..n]).await;
            });
        }
    });

    let mut stream = tokio::net::TcpStream::connect(addr).await?;
    stream.write_all(b"ping").await?;
    let mut buf = [0; 10];
    let n = stream.read(&mut buf[..]).await?;
    println!("{}", String::from_utf8_lossy(&buf[..n]));
    Ok(())
}
