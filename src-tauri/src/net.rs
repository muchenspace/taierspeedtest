use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyInit};
use des::Des;
use serde::{Deserialize, Serialize};

pub const CONFIG_HOST: &str = "dlcv2.cnspeedtest.com";
pub const CONFIG_PORT: u16 = 8043;
pub const CONFIG_PATH: &str = "/TaierAndroid/Config/serverlist_encrypt.json";

const DES_KEY: &[u8; 8] = b"dw!@#$%^";
pub const PKG: &str = "com.cnspeedtest.globalspeed";

pub const UA_DALVIK: &str =
    "Dalvik/2.1.0 (Linux; U; Android 14; PCRT00 Build/UKQ1.230917.001)";
pub const UA_BROWSER: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/100.0.4896.60 Safari/537.36";
pub const UA_UPLOAD: &str =
    "Dalvik/1.6.0 (Linux; U; Android 4.2.2; GT-I9505 Build/JDQ39)";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub pname: String,
    pub oper: String,
    pub hostname: String,
    pub hostip: String,
    pub port: u16,
}

#[derive(Deserialize)]
struct RawNode {
    #[serde(default)]
    hostip: String,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    pname: String,
    #[serde(default)]
    oper: String,
    #[serde(default)]
    location: String,
    #[serde(default, deserialize_with = "de_port")]
    port: u16,
}

fn de_port<'de, D>(d: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let v = serde_json::Value::deserialize(d).map_err(D::Error::custom)?;
    match v {
        serde_json::Value::String(s) => s.trim().parse().map_err(D::Error::custom),
        serde_json::Value::Number(n) => n
            .as_u64()
            .map(|x| x as u16)
            .ok_or_else(|| D::Error::custom("port 超出范围")),
        serde_json::Value::Null => Ok(65499),
        _ => Err(D::Error::custom("port 类型非法")),
    }
}

impl RawNode {
    fn into_node(self) -> Node {
        Node {

            pname: if self.pname.is_empty() {
                self.location.clone()
            } else {
                self.pname.clone()
            },
            oper: self.oper,
            hostname: if self.hostname.is_empty() {
                self.hostip.clone()
            } else {
                self.hostname
            },
            hostip: self.hostip,
            port: if self.port == 0 { 65499 } else { self.port },
        }
    }
}

fn parse_nodes(text: &str) -> Vec<Node> {
    let raw: Vec<RawNode> = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    raw.into_iter()
        .map(RawNode::into_node)
        .filter(|n| !n.hostip.is_empty())
        .collect()
}

pub fn refresh_nodes() -> Result<Vec<Node>, String> {
    let client = reqwest::blocking::Client::builder()

        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(25))
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let url = format!("https://{}:{}{}", CONFIG_HOST, CONFIG_PORT, CONFIG_PATH);
    let text = client
        .get(&url)
        .send()
        .map_err(|e| format!("节点列表请求失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("节点列表返回异常: {e}"))?
        .text()
        .map_err(|e| format!("节点列表读取失败: {e}"))?;

    let hexs: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hexs.is_empty() {
        return Err("节点列表为空".into());
    }
    let cipher_text = hex::decode(&hexs).map_err(|e| format!("密文解码失败: {e}"))?;
    let plain = des_decrypt_ecb(&cipher_text).map_err(|e| format!("解密失败: {e}"))?;
    let plain_text = String::from_utf8_lossy(&plain);

    let nodes = parse_nodes(&plain_text);
    if nodes.is_empty() {
        return Err("未解析到任何节点".into());
    }
    Ok(nodes)
}

fn des_decrypt_ecb(data: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Des::new_from_slice(DES_KEY).map_err(|e| e.to_string())?;
    let mut buf = data.to_vec();
    let out = cipher
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| format!("{e:?}"))?;
    Ok(out.to_vec())
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn pseudo_rand() -> u64 {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(12345);
    let mut x = n.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x
}

fn make_imei() -> String {
    let base = (now_millis() as u64) % 100_000_000_000;
    format!("{:010}{:05}", base, pseudo_rand() % 100_000)
}

fn md5_hex(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

pub fn make_token(imei: &str, ts: &str) -> String {
    let a = md5_hex(&format!("model=Android&imei={imei}"));
    let b = md5_hex(&format!("stime={ts}&band=200&rand=12345555"));
    md5_hex(&format!("{a}{b}"))
}

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

pub fn tcp_connect(ip: &str, port: u16, timeout: Duration) -> std::io::Result<TcpStream> {
    let addr: SocketAddr = format!("{ip}:{port}")
        .parse()
        .map_err(|_| std::io::Error::new(ErrorKind::InvalidInput, "无效的节点地址"))?;
    let s = TcpStream::connect_timeout(&addr, timeout)?;
    s.set_nodelay(true)?;
    Ok(s)
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn dechunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        let Some(rel) = find_sub(&body[i..], b"\r\n") else {
            break;
        };
        let e = i + rel;
        let size_text = String::from_utf8_lossy(&body[i..e]);
        let size_text = size_text.split(';').next().unwrap_or("").trim().to_string();
        let Ok(sz) = usize::from_str_radix(&size_text, 16) else {
            break;
        };
        if sz == 0 {
            break;
        }
        let data = e + 2;
        if data + sz > body.len() {
            break;
        }
        out.extend_from_slice(&body[data..data + sz]);
        i = data + sz + 2;
    }
    out
}

fn parse_http(buf: &[u8]) -> (u16, Vec<u8>) {
    let Some(pos) = find_sub(buf, b"\r\n\r\n") else {
        return (0, Vec::new());
    };
    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let mut body = buf[pos + 4..].to_vec();
    if head.to_ascii_lowercase().contains("chunked") {
        body = dechunk(&body);
    }
    (status, body)
}

pub fn http_small(
    ip: &str,
    port: u16,
    req: &[u8],
    timeout: Duration,
) -> std::io::Result<HttpResponse> {
    let mut s = tcp_connect(ip, port, timeout)?;

    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;

    s.write_all(req)?;

    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut tmp = [0u8; 65536];
    loop {
        match s.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > 4 * 1024 * 1024 {
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                break
            }
            Err(e) => return Err(e),
        }
    }
    let (status, body) = parse_http(&buf);
    Ok(HttpResponse { status, body })
}

pub fn req_get(ip: &str, port: u16, path: &str, ua: &str) -> Vec<u8> {
    format!(
        "GET {path} HTTP/1.1\r\nHost: {ip}:{port}\r\nConnection: close\r\n\
User-Agent: {ua}\r\n\r\n"
    )
    .into_bytes()
}

fn ipv4_to_u32(ip: &str) -> Option<u32> {
    let v: std::net::Ipv4Addr = ip.trim().parse().ok()?;
    Some(u32::from_ne_bytes(v.octets()))
}

#[cfg(windows)]
fn icmp_ping_once(ip: &str, timeout_ms: u32) -> Option<f64> {
    use core::ffi::c_void;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY,
    };

    let addr = ipv4_to_u32(ip)?;
    let payload = [b'a'; 32];

    unsafe {
        let handle: HANDLE = IcmpCreateFile();

        if handle.is_null() || handle as isize == -1 {
            return None;
        }

        let mut reply = vec![0u8; core::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 32];
        let n = IcmpSendEcho(
            handle,
            addr,
            payload.as_ptr() as *const c_void,
            payload.len() as u16,
            core::ptr::null(),
            reply.as_mut_ptr() as *mut c_void,
            reply.len() as u32,
            timeout_ms,
        );
        IcmpCloseHandle(handle);

        if n == 0 {
            return None;
        }
        let rep = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);

        if rep.Status != 0 {
            return None;
        }
        Some(rep.RoundTripTime as f64)
    }
}

#[cfg(not(windows))]
fn icmp_ping_once(_ip: &str, _timeout_ms: u32) -> Option<f64> {
    None
}

pub fn measure_latency(node: &Node, on_step: &mut dyn FnMut(f64)) -> f64 {
    let mut rtts: Vec<f64> = Vec::new();

    for _ in 0..6 {
        if let Some(ms) = icmp_ping_once(&node.hostip, 1200) {
            rtts.push(ms);
            on_step(ms);
        } else {
            on_step(-1.0);
        }
        std::thread::sleep(Duration::from_millis(120));
    }

    if rtts.is_empty() {
        return 0.0;
    }
    rtts.iter().sum::<f64>() / rtts.len() as f64
}

pub fn enqueue(node: &Node) -> Result<String, String> {
    let imei = make_imei();
    let ts = now_secs().to_string();
    let token = make_token(&imei, &ts);
    let path = format!(
        "/speed/dovalid?key=&flag=true&bandwidth=200&model=Android&imei={imei}\
&time={ts}&app=globalspeed&token={token}&pkg={PKG}"
    );
    let req = req_get(&node.hostip, node.port, &path, UA_DALVIK);

    let resp = http_small(&node.hostip, node.port, &req, Duration::from_secs(6))
        .map_err(|e| format!("连接节点失败: {e}"))?;

    if !(200..300).contains(&resp.status) {
        return Err(format!("入队失败: HTTP {} (节点可能限流)", resp.status));
    }
    let body = String::from_utf8_lossy(&resp.body).trim().to_string();
    if body.is_empty() {
        return Err("入队失败: 空响应".into());
    }
    if !body.starts_with("1-") {
        let code = body.chars().next().unwrap_or('?');
        let why = match code {
            '0' => "被节点拒绝",
            '2' => "节点忙",
            '-' => "被限流(-1)，稍后重试或换节点",
            _ => "未知响应",
        };
        return Err(format!("入队失败: {why}"));
    }
    let uuid = body[2..].to_string();
    if uuid.is_empty() || uuid.len() >= 64 || !uuid.chars().all(|c| c.is_ascii_graphic()) {
        return Err("入队失败: 节点返回无效标识".into());
    }
    Ok(uuid)
}

pub fn dequeue(node: &Node, uuid: &str) {
    let req = format!(
        "POST /speed/dovalid?key={uuid} HTTP/1.1\r\nHost: {}:{}\r\nCharset: utf-8\r\n\
Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 0\r\n\
Connection: close\r\n\r\n",
        node.hostip, node.port
    );
    let _ = http_small(
        &node.hostip,
        node.port,
        req.as_bytes(),
        Duration::from_secs(4),
    );
}
