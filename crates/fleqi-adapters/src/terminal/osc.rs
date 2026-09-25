//! 从 PTY 输出流中剥离 Fleqi 私有 OSC 7331 序列（`ESC ] 7331 ; payload BEL|ST`），
//! 其余字节原样转发给屏幕解析与订阅者。任意程序输出的其它 OSC（含 OSC 7）不作为空闲证据。

use super::integration::OSC_CODE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integration {
    Prompt { cwd: String },
    Preexec,
    EditLen(usize),
    Unknown(String),
}

#[derive(Default)]
pub struct OscExtractor {
    pending: Vec<u8>,
}

impl OscExtractor {
    /// 返回（转发字节，解析到的集成消息）。跨读取边界的半截序列保留在内部缓冲。
    pub fn feed(&mut self, input: &[u8]) -> (Vec<u8>, Vec<Integration>) {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(input);
        let prefix = format!("\x1b]{OSC_CODE};").into_bytes();
        let mut out = Vec::with_capacity(data.len());
        let mut messages = Vec::new();
        let mut i = 0;
        while i < data.len() {
            if data[i] == 0x1b {
                let rest = &data[i..];
                if rest.len() < prefix.len() {
                    if prefix.starts_with(rest) {
                        self.pending = rest.to_vec();
                        return (out, messages);
                    }
                } else if rest.starts_with(&prefix) {
                    let body_start = i + prefix.len();
                    let mut j = body_start;
                    let mut end = None;
                    while j < data.len() {
                        if data[j] == 0x07 {
                            end = Some((j, 1));
                            break;
                        }
                        if data[j] == 0x1b && j + 1 < data.len() && data[j + 1] == b'\\' {
                            end = Some((j, 2));
                            break;
                        }
                        j += 1;
                    }
                    match end {
                        Some((e, len)) => {
                            let body = String::from_utf8_lossy(&data[body_start..e]).into_owned();
                            messages.push(parse_body(&body));
                            i = e + len;
                            continue;
                        }
                        None => {
                            if data.len() - i > 64 * 1024 {
                                // 异常长的伪序列：放弃解析，原样转发。
                                out.extend_from_slice(rest);
                                return (out, messages);
                            }
                            self.pending = rest.to_vec();
                            return (out, messages);
                        }
                    }
                }
            }
            out.push(data[i]);
            i += 1;
        }
        (out, messages)
    }
}

fn parse_body(body: &str) -> Integration {
    if let Some(rest) = body.strip_prefix("prompt;cwd=") {
        return Integration::Prompt {
            cwd: decode_hex(rest),
        };
    }
    if body == "preexec" {
        return Integration::Preexec;
    }
    if let Some(rest) = body.strip_prefix("edit;len=") {
        return Integration::EditLen(rest.trim().parse().unwrap_or(0));
    }
    Integration::Unknown(body.to_owned())
}

fn decode_hex(hex: &str) -> String {
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
        })
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_messages_and_forwards_other_bytes_across_chunks() {
        let mut ex = OscExtractor::default();
        let (out, msgs) = ex.feed(b"hello\x1b]7331;pre");
        assert_eq!(out, b"hello");
        assert!(msgs.is_empty());
        let (out, msgs) = ex.feed(b"exec\x07world\x1b]7331;edit;len=3\x1b\\!");
        assert_eq!(out, b"world!");
        assert_eq!(msgs, vec![Integration::Preexec, Integration::EditLen(3)]);
        let hex = "2f746d702f61";
        let (_, msgs) = ex.feed(format!("\x1b]7331;prompt;cwd={hex}\x07").as_bytes());
        assert_eq!(
            msgs,
            vec![Integration::Prompt {
                cwd: "/tmp/a".into()
            }]
        );
        // 其它 OSC（如 OSC 7）原样转发，不当作集成消息。
        let (out, msgs) = ex.feed(b"\x1b]7;file://host/tmp\x07x");
        assert_eq!(out, b"\x1b]7;file://host/tmp\x07x");
        assert!(msgs.is_empty());
    }
}
