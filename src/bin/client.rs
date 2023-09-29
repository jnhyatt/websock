use async_std::io::ReadExt;
use async_std::net::TcpListener;
use futures::stream::StreamExt;

#[derive(Debug)]
pub enum ParseError {
    Unfinished,
    ReservedOpCode,
    ReservedBit,
}

#[derive(Debug)]
pub enum OpCode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

#[derive(Debug)]
pub struct Frame {
    is_last_frag: bool,
    op_code: OpCode,
    masking_key: Option<u32>,
    payload: Vec<u8>,
}

pub fn parse_frame(bytes: &[u8]) -> Result<(usize, Frame), ParseError> {
    let byte0 = bytes.get(0).ok_or(ParseError::Unfinished)?;
    let byte1 = bytes.get(1).ok_or(ParseError::Unfinished)?;
    let fin = byte0 & 0b1000000 != 0;
    if byte0 & 0b01110000 != 0 {
        return Err(ParseError::ReservedBit);
    }
    let op = byte0 & 0b00001111;
    let op = match op {
        0x0 => OpCode::Continuation,
        0x1 => OpCode::Text,
        0x2 => OpCode::Binary,
        0x8 => OpCode::Close,
        0x9 => OpCode::Ping,
        0xA => OpCode::Pong,
        _ => return Err(ParseError::ReservedOpCode),
    };
    let is_masked = byte1 & 0b10000000 != 0;
    let len = byte1 & 0b01111111;
    let payload_len = match len {
        126 => {
            let high = *bytes.get(2).ok_or(ParseError::Unfinished)?;
            let low = *bytes.get(3).ok_or(ParseError::Unfinished)?;
            (high as u64) << 8 | low as u64
        }
        127 => {
            if bytes.len() < 10 {
                return Err(ParseError::Unfinished);
            }
            let mut result = [0; 8];
            result.copy_from_slice(&bytes[2..10]);
            u64::from_be_bytes(result)
        }
        _ => len as u64,
    };
    let mask_offset = 2 + match len {
        126 => 2,
        127 => 8,
        _ => 0,
    };
    let payload_offset = mask_offset + if is_masked { 4 } else { 0 };
    let mask = if is_masked {
        if bytes.len() < mask_offset + 4 {
            return Err(ParseError::Unfinished);
        }
        let mut result = [0; 4];
        result.copy_from_slice(&bytes[mask_offset..]);
        Some(u32::from_be_bytes(result))
    } else {
        None
    };
    let frame_end = payload_offset + payload_len as usize;
    if bytes.len() < frame_end {
        return Err(ParseError::Unfinished);
    }
    Ok((
        frame_end,
        Frame {
            is_last_frag: fin,
            op_code: op,
            masking_key: mask,
            payload: (&bytes[payload_offset..frame_end]).to_vec(),
        },
    ))
}

fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let port = TcpListener::bind("localhost:3000").await.unwrap();
        port.incoming()
            .map(Result::unwrap)
            .for_each_concurrent(None, |mut conn| async move {
                let mut current_parse = Vec::new();
                loop {
                    let mut buffer = vec![0; 256];
                    conn.read(&mut buffer);
                    current_parse.append(&mut buffer);
                    match parse_frame(&current_parse) {
                        Ok((n, frame)) => {
                            current_parse = current_parse[n..].to_vec();
                            println!("{frame:?}");
                        }
                        Err(ParseError::Unfinished) => continue,
                        Err(e) => {
                            println!("{e:?}");
                            break;
                        }
                    }
                }
            })
            .await;
    });
}
