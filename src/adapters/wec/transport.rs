use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    net::TcpStream,
    path::PathBuf,
    time::Duration,
};

use rand::{distributions::Alphanumeric, Rng};
use reqwest::blocking::Client;
use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    Error as WsError, Message, WebSocket,
};

use super::ddp::{parse_sockjs_packet, SockJsPacket};

const SOCKJS_BASE_URL: &str = "wss://livetiming.alkamelsystems.com/sockjs";
const SOCKJS_INFO_URL: &str = "https://livetiming.alkamelsystems.com/sockjs/info";
const ORIGIN_URL: &str = "https://livetiming.alkamelsystems.com";

pub struct SockJsTransport {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    ddp_dump: Option<File>,
}

impl SockJsTransport {
    pub fn connect_from_env() -> Result<Self, String> {
        sockjs_info_preflight();

        let ws_url = random_sockjs_url();
        let request = build_request(&ws_url)?;
        let (mut socket, _) =
            connect(request).map_err(|err| format!("WEC websocket connect failed: {err}"))?;

        set_socket_timeout(&mut socket);

        let ddp_dump = std::env::var("WEC_DDP_DUMP_PATH")
            .ok()
            .map(PathBuf::from)
            .and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok());

        Ok(Self { socket, ddp_dump })
    }

    pub fn read_packet(&mut self) -> Result<Option<SockJsPacket>, String> {
        match self.socket.read() {
            Ok(Message::Text(text)) => {
                self.dump("in", &text);
                Ok(Some(parse_sockjs_packet(&text)))
            }
            Ok(Message::Binary(data)) => {
                let Ok(text) = String::from_utf8(data.to_vec()) else {
                    return Ok(None);
                };
                self.dump("in", &text);
                Ok(Some(parse_sockjs_packet(&text)))
            }
            Ok(Message::Ping(data)) => {
                self.socket
                    .send(Message::Pong(data))
                    .map_err(|err| format!("WEC ping/pong handling failed: {err}"))?;
                Ok(None)
            }
            Ok(Message::Pong(_)) => Ok(None),
            Ok(Message::Close(_)) => Ok(Some(SockJsPacket::Close)),
            Ok(Message::Frame(_)) => Ok(None),
            Err(WsError::Io(err))
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                Ok(None)
            }
            Err(err) => Err(format!("WEC websocket read failed: {err}")),
        }
    }

    pub fn send_ddp(&mut self, ddp_message: String) -> Result<(), String> {
        let sockjs_payload =
            serde_json::to_string(&vec![ddp_message]).unwrap_or_else(|_| "[]".to_string());
        self.dump("out", &sockjs_payload);
        self.socket
            .send(Message::Text(sockjs_payload))
            .map_err(|err| format!("WEC websocket send failed: {err}"))
    }

    fn dump(&mut self, direction: &str, raw: &str) {
        let Some(file) = self.ddp_dump.as_mut() else {
            return;
        };
        let _ = writeln!(file, "[{direction}] {raw}");
    }
}

fn random_sockjs_url() -> String {
    let mut rng = rand::thread_rng();
    let server_id = rng.gen_range(0..1000);
    let session_id: String = rng
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    format!("{SOCKJS_BASE_URL}/{server_id:03}/{session_id}/websocket")
}

fn build_request(url: &str) -> Result<tungstenite::handshake::client::Request, String> {
    let mut request = url
        .into_client_request()
        .map_err(|err| format!("failed to build websocket request: {err}"))?;
    request
        .headers_mut()
        .insert(ORIGIN, HeaderValue::from_static(ORIGIN_URL));
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
    Ok(request)
}

fn set_socket_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    }
}

fn sockjs_info_preflight() {
    let client = match Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(client) => client,
        Err(_) => return,
    };

    let cb: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();

    let _ = client
        .get(SOCKJS_INFO_URL)
        .query(&[("cb", cb)])
        .header("Referer", "https://livetiming.alkamelsystems.com/fiawec")
        .send();
}
