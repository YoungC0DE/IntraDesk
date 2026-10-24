use crate::{tcp::FramedStream, ResultType, Stream};
use std::net::SocketAddr;
use tokio::net::ToSocketAddrs;

#[inline]
pub fn check_port<T: std::string::ToString>(host: T, port: i32) -> String {
    let host = host.to_string();
    if crate::is_ipv6_str(&host) {
        if host.starts_with('[') {
            return host;
        }
        return format!("[{host}]:{port}");
    }
    if !host.contains(':') {
        return format!("{host}:{port}");
    }
    host
}

#[inline]
pub fn increase_port<T: std::string::ToString>(host: T, offset: i32) -> String {
    let host = host.to_string();
    if crate::is_ipv6_str(&host) {
        if host.starts_with('[') {
            let tmp: Vec<&str> = host.split("]:").collect();
            if tmp.len() == 2 {
                let port: i32 = tmp[1].parse().unwrap_or(0);
                if port > 0 {
                    return format!("{}]:{}", tmp[0], port + offset);
                }
            }
        }
    } else if host.contains(':') {
        let tmp: Vec<&str> = host.split(':').collect();
        if tmp.len() == 2 {
            let port: i32 = tmp[1].parse().unwrap_or(0);
            if port > 0 {
                return format!("{}:{}", tmp[0], port + offset);
            }
        }
    }
    host
}

/// Connects straight to `target` over TCP.
#[inline]
pub async fn connect_tcp<T: ToSocketAddrs + std::fmt::Display>(
    target: T,
    ms_timeout: u64,
) -> ResultType<Stream> {
    connect_tcp_local(target, None, ms_timeout).await
}

/// Connects straight to `target` over TCP, from `local` when given.
pub async fn connect_tcp_local<T: ToSocketAddrs + std::fmt::Display>(
    target: T,
    local: Option<SocketAddr>,
    ms_timeout: u64,
) -> ResultType<Stream> {
    Ok(Stream::Tcp(
        FramedStream::new(target, local, ms_timeout).await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_port() {
        assert_eq!(check_port("[1:2]:12", 32), "[1:2]:12");
        assert_eq!(check_port("1:2", 32), "[1:2]:32");
        assert_eq!(check_port("z1:2", 32), "z1:2");
        assert_eq!(check_port("1.1.1.1", 32), "1.1.1.1:32");
        assert_eq!(check_port("1.1.1.1:32", 32), "1.1.1.1:32");
        assert_eq!(check_port("test.com:32", 0), "test.com:32");
        assert_eq!(increase_port("[1:2]:12", 1), "[1:2]:13");
        assert_eq!(increase_port("1.2.2.4:12", 1), "1.2.2.4:13");
        assert_eq!(increase_port("1.2.2.4", 1), "1.2.2.4");
        assert_eq!(increase_port("test.com", 1), "test.com");
        assert_eq!(increase_port("test.com:13", 4), "test.com:17");
        assert_eq!(increase_port("1:13", 4), "1:13");
        assert_eq!(increase_port("22:1:13", 4), "22:1:13");
        assert_eq!(increase_port("z1:2", 1), "z1:3");
    }
}
