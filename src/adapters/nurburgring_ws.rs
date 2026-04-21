use std::{net::TcpStream, time::Duration};

use tungstenite::{
    client::IntoClientRequest,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    WebSocket,
};

pub(crate) fn build_request(
    ws_url: &str,
    origin: &'static str,
) -> tungstenite::handshake::client::Request {
    let mut request = ws_url
        .into_client_request()
        .expect("failed to create websocket request");
    request
        .headers_mut()
        .insert(ORIGIN, HeaderValue::from_static(origin));
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
    request
}

pub(crate) fn set_socket_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    const READ_TIMEOUT: Duration = Duration::from_secs(2);
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        }
        MaybeTlsStream::Rustls(stream) => {
            let _ = stream.get_mut().set_read_timeout(Some(READ_TIMEOUT));
        }
        _ => {}
    }
}
