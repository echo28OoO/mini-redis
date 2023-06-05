use bytes::{Bytes, BytesMut, Buf};
use tokio::net::TcpStream;
use mini_redis::{Frame, Result};
use mini_redis::frame::Error::Incomplete;
use tokio::io::{self, AsyncWriteExt};
use std::io::Cursor;

enum Frame {
    Simple(String),
    Error(String),
    Integer(u64),
    Bulk(Bytes),
    Null,
    Array(Vec<Frame>),
}

pub struct Connection {
    stream: BufWriter<TcpStream>,
    buffer: BytesMut,
}

impl Connection {

    pub fn new(stream: TcpStream) -> Connection {
        Connection {
            stream: BufWriter::new(stream),
            buffer: BytesMut::with_capacity(4096),
        }
    }

    pub async fn read_frame(&mut self)
    -> Result<Option<Frame>>
    {
        loop {
            if let Some(frame) = self.parse_frame()? {
                return Ok(Some(frame));
            }
    
            // 确保缓冲区长度足够
            if self.buffer.len() == self.cursor {
                // 若不够，需要增加缓冲区长度
                self.buffer.resize(self.cursor * 2, 0);
            }
    
            // 从游标位置开始将数据读入缓冲区
            let n = self.stream.read(
                &mut self.buffer[self.cursor..]).await?;
    
            if 0 == n {
                if self.cursor == 0 {
                    return Ok(None);
                } else {
                    return Err("connection reset by peer".into());
                }
            } else {
                // 更新游标位置
                self.cursor += n;
            }
        }
    }

    /// 将帧写入到连接中
    pub async fn write_frame(&mut self, frame: &Frame)
        -> Result<()>
        {
            match frame {
                Frame::Simple(val) => {
                    self.stream.write_u8(b'+').await?;
                    self.stream.write_all(val.as_bytes()).await?;
                    self.stream.write_all(b"\r\n").await?;
                }
                Frame::Error(val) => {
                    self.stream.write_u8(b'-').await?;
                    self.stream.write_all(val.as_bytes()).await?;
                    self.stream.write_all(b"\r\n").await?;
                }
                Frame::Integer(val) => {
                    self.stream.write_u8(b':').await?;
                    self.write_decimal(*val).await?;
                }
                Frame::Null => {
                    self.stream.write_all(b"$-1\r\n").await?;
                }
                Frame::Bulk(val) => {
                    let len = val.len();
        
                    self.stream.write_u8(b'$').await?;
                    self.write_decimal(len as u64).await?;
                    self.stream.write_all(val).await?;
                    self.stream.write_all(b"\r\n").await?;
                }
                Frame::Array(_val) => unimplemented!(),
            }
        
            self.stream.flush().await;
        
            Ok(())
        }

    // 帧解析
    fn parse_frame(&mut self)
    -> Result<Option<Frame>>
    {
        // 创建 `T: Buf` 类型
        let mut buf = Cursor::new(&self.buffer[..]);

        // 检查是否读取了足够解析出一个帧的数据
        match Frame::check(&mut buf) {
            Ok(_) => {
                // 获取组成该帧的字节数
                let len = buf.position() as usize;

                // 在解析开始之前，重置内部的游标位置
                buf.set_position(0);

                // 解析帧
                let frame = Frame::parse(&mut buf)?;

                // 解析完成，将缓冲区该帧的数据移除
                self.buffer.advance(len);

                // 返回解析出的帧
                Ok(Some(frame))
            }
            // 缓冲区的数据不足以解析出一个完整的帧
            Err(Incomplete) => Ok(None),
            // 遇到一个错误
            Err(e) => Err(e.into()),
        }
    }
}