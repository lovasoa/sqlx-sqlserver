use sqlx_core::Error;
use tokio::net::UdpSocket;
use tokio::time::{timeout, Duration};

const SSRP_PORT: u16 = 1434;
const CLNT_UCAST_INST: u8 = 0x04;
const SVR_RESP: u8 = 0x05;
const SSRP_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) async fn resolve_instance_port(server: &str, instance: &str) -> Result<u16, Error> {
    let mut request = Vec::with_capacity(1 + instance.len() + 1);
    request.push(CLNT_UCAST_INST);
    request.extend_from_slice(instance.as_bytes());
    request.push(0);

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.send_to(&request, (server, SSRP_PORT)).await?;

    let mut buffer = [0u8; 1024];
    let bytes_read = timeout(SSRP_TIMEOUT, socket.recv(&mut buffer))
        .await
        .map_err(|_| {
            Error::Protocol(format!(
                "SSRP request to {server} for instance {instance} timed out"
            ))
        })??;

    parse_ssrp_response(&buffer[..bytes_read], instance)
}

fn parse_ssrp_response(input: &[u8], instance: &str) -> Result<u16, Error> {
    if input.len() < 3 {
        return Err(Error::Protocol(format!(
            "SSRP response too short: {} bytes",
            input.len()
        )));
    }

    if input[0] != SVR_RESP {
        return Err(Error::Protocol(format!(
            "invalid SSRP response type: expected 0x05, got 0x{:02x}",
            input[0]
        )));
    }

    let response_size = usize::from(u16::from_le_bytes([input[1], input[2]]));
    let response_end = response_size
        .checked_add(3)
        .ok_or_else(|| Error::Protocol("SSRP response size overflow".to_owned()))?;
    let response = input.get(3..response_end).ok_or_else(|| {
        Error::Protocol(format!(
            "SSRP response size mismatch: expected {response_end} bytes, got {}",
            input.len()
        ))
    })?;
    let response = String::from_utf8_lossy(response);

    find_instance_tcp_port(&response, instance)
}

fn find_instance_tcp_port(data: &str, instance_name: &str) -> Result<u16, Error> {
    for instance_data in data.split(";;").filter(|segment| !segment.is_empty()) {
        let mut tokens = instance_data.split(';');
        let mut found_instance_name = None;
        let mut tcp_port = None;

        while let Some(key) = tokens.next() {
            let value = tokens.next();

            match key {
                "InstanceName" => found_instance_name = value,
                "tcp" => tcp_port = value.and_then(|value| value.parse::<u16>().ok()),
                _ => {}
            }
        }

        if found_instance_name.is_some_and(|name| name.eq_ignore_ascii_case(instance_name)) {
            return tcp_port.ok_or_else(|| {
                Error::Protocol(format!(
                    "SQL Server instance `{instance_name}` found but no TCP port was advertised"
                ))
            });
        }
    }

    Err(Error::Protocol(format!(
        "SQL Server instance `{instance_name}` was not found in SSRP response"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssrp_response_for_requested_instance() {
        let body = b"ServerName;SRV;InstanceName;SQLEXPRESS;IsClustered;No;Version;16.0;tcp;1435;;";
        let mut response = vec![SVR_RESP];
        response.extend_from_slice(&(body.len() as u16).to_le_bytes());
        response.extend_from_slice(body);

        assert_eq!(1435, parse_ssrp_response(&response, "sqlexpress").unwrap());
    }

    #[test]
    fn finds_tcp_port_among_multiple_instances() {
        let body = "\
            ServerName;SRV;InstanceName;FIRST;tcp;1433;;\
            ServerName;SRV;InstanceName;SECOND;tcp;1434;;\
        ";

        assert_eq!(1434, find_instance_tcp_port(body, "SECOND").unwrap());
    }

    #[test]
    fn finds_instance_case_insensitively() {
        let body = "ServerName;SRV;InstanceName;SqLeXpReSs;tcp;1435;;";

        assert_eq!(1435, find_instance_tcp_port(body, "sqlexpress").unwrap());
    }

    #[test]
    fn rejects_ssrp_response_without_matching_instance() {
        let body = b"ServerName;SRV;InstanceName;OTHER;tcp;1435;;";
        let mut response = vec![SVR_RESP];
        response.extend_from_slice(&(body.len() as u16).to_le_bytes());
        response.extend_from_slice(body);

        assert!(parse_ssrp_response(&response, "SQLEXPRESS").is_err());
    }

    #[test]
    fn rejects_instance_without_tcp_port() {
        let body = "ServerName;SRV;InstanceName;SQLEXPRESS;np;\\\\server\\pipe;;";
        let error = find_instance_tcp_port(body, "SQLEXPRESS").unwrap_err();

        assert!(error.to_string().contains("no TCP port"));
    }

    #[test]
    fn rejects_too_short_response() {
        let error = parse_ssrp_response(&[SVR_RESP, 0], "SQLEXPRESS").unwrap_err();

        assert!(error.to_string().contains("too short"));
    }

    #[test]
    fn rejects_invalid_response_type() {
        let error = parse_ssrp_response(&[0x04, 0, 0], "SQLEXPRESS").unwrap_err();

        assert!(error.to_string().contains("invalid SSRP response type"));
    }

    #[test]
    fn rejects_response_size_mismatch() {
        let error = parse_ssrp_response(&[SVR_RESP, 10, 0, b'a'], "SQLEXPRESS").unwrap_err();

        assert!(error.to_string().contains("size mismatch"));
    }
}
