use std::{
    io::{Read, Write},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[cfg(unix)]
pub fn query_background_color(output: &mut impl Write, timeout: Duration) -> Option<TerminalColor> {
    use std::os::fd::AsRawFd;

    let stdin = std::io::stdin();
    let fd = stdin.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return None;
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return None;
    }

    let result = query_background_color_inner(output, timeout);
    let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    result
}

#[cfg(not(unix))]
pub fn query_background_color(
    _output: &mut impl Write,
    _timeout: Duration,
) -> Option<TerminalColor> {
    None
}

fn query_background_color_inner(
    output: &mut impl Write,
    timeout: Duration,
) -> Option<TerminalColor> {
    output.write_all(b"\x1b]11;?\x07").ok()?;
    output.flush().ok()?;

    let deadline = Instant::now() + timeout;
    let mut stdin = std::io::stdin();
    let mut response = Vec::new();
    let mut buffer = [0_u8; 128];

    while Instant::now() < deadline {
        match stdin.read(&mut buffer) {
            Ok(0) => thread::sleep(Duration::from_millis(2)),
            Ok(bytes) => {
                response.extend_from_slice(&buffer[..bytes]);
                if response.ends_with(b"\x07") || response.ends_with(b"\x1b\\") {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return None,
        }
    }

    parse_osc_11_response(&String::from_utf8_lossy(&response))
}

fn parse_osc_11_response(response: &str) -> Option<TerminalColor> {
    let start = response.find("rgb:")? + "rgb:".len();
    let rgb = response[start..]
        .trim_end_matches('\u{7}')
        .trim_end_matches("\u{1b}\\");
    let mut parts = rgb.split('/');
    let r = scale_xterm_component(parts.next()?)?;
    let g = scale_xterm_component(parts.next()?)?;
    let b = scale_xterm_component(parts.next()?)?;
    Some(TerminalColor { r, g, b })
}

fn scale_xterm_component(component: &str) -> Option<u8> {
    let hex = component
        .chars()
        .take_while(|character| character.is_ascii_hexdigit())
        .collect::<String>();
    if hex.is_empty() {
        return None;
    }
    let value = u32::from_str_radix(&hex, 16).ok()?;
    let max = (1_u32 << (hex.len() * 4)).saturating_sub(1).max(1);
    Some(((value * 255) / max) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_four_digit_osc_11_response() {
        let parsed = parse_osc_11_response("\x1b]11;rgb:1a1a/2020/2f2f\x07").unwrap();
        assert_eq!(
            parsed,
            TerminalColor {
                r: 26,
                g: 32,
                b: 47
            }
        );
    }

    #[test]
    fn parses_two_digit_osc_11_response() {
        let parsed = parse_osc_11_response("\x1b]11;rgb:fa/ef/d7\x1b\\").unwrap();
        assert_eq!(
            parsed,
            TerminalColor {
                r: 250,
                g: 239,
                b: 215
            }
        );
    }
}
