//! ZMODEM 协议实现（简化版，用于测试）

pub const ZPAD: u8 = 0x80;
pub const ZDLE: u8 = 0x18;
pub const ZRQINIT: u8 = 0x64;
pub const ZRINIT: u8 = 0x62;
pub const ZFILE: u8 = 0x63;
pub const ZACK: u8 = 0x60;
pub const ZDATA: u8 = 0x66;
pub const ZEOF: u8 = 0x65;

/// 检测 ZRQINIT 包
pub fn is_zrqinit(data: &[u8]) -> bool {
    for i in 0..data.len().saturating_sub(3) {
        // **ZDLE ZRQINIT
        if data[i] == ZPAD && 
           data[i+1] == ZPAD && 
           data[i+2] == ZDLE && 
           data[i+3] == ZRQINIT {
            return true;
        }
        // 0x80 0x80 0x18 0x64
        if i + 3 < data.len() &&
           data[i] == 0x80 && 
           data[i+1] == 0x80 && 
           data[i+2] == 0x18 && 
           data[i+3] == 0x64 {
            return true;
        }
    }
    false
}

/// 编码 ZRINIT 包
pub fn encode_zrinit() -> Vec<u8> {
    encode_header(ZRINIT, [0x40, 0x00, 0x00, 0x00])
}

/// 编码 ZACK 包
pub fn encode_zack(position: u64) -> Vec<u8> {
    let pos_bytes = position.to_be_bytes();
    encode_header(ZACK, [pos_bytes[4], pos_bytes[5], pos_bytes[6], pos_bytes[7]])
}

/// 编码头部包
fn encode_header(packet_type: u8, header_data: [u8; 4]) -> Vec<u8> {
    let mut result = Vec::new();
    
    // **
    result.push(ZPAD);
    result.push(ZPAD);
    
    // ZDLE + 包类型
    result.push(ZDLE);
    result.push(packet_type);
    
    // 头部数据（带 ZDLE 转义）
    for &b in &header_data {
        result.push(ZDLE);
        result.push(b ^ 0x40);
    }
    
    // CRC-16 (简化，全 0)
    result.push(ZDLE);
    result.push(0x40);
    result.push(ZDLE);
    result.push(0x40);
    
    result
}

/// 解析 ZFILE 包，返回文件名
pub fn parse_zfile(data: &[u8]) -> Option<String> {
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == ZPAD && 
           data[i+1] == ZPAD && 
           data[i+2] == ZDLE && 
           data[i+3] == ZFILE {
            
            // 解析头部
            let mut j = i + 4;
            let mut header_bytes = [0u8; 4];
            let mut idx = 0;
            
            while j < data.len() && idx < 4 {
                if data[j] == ZDLE && j + 1 < data.len() {
                    header_bytes[idx] = data[j+1] ^ 0x40;
                    idx += 1;
                    j += 2;
                } else {
                    j += 1;
                    idx += 1;
                }
            }
            
            if idx < 4 {
                return None;
            }
            
            // 提取文件名
            let mut filename = String::new();
            for &b in &header_bytes {
                if b == 0 {
                    break;
                }
                if let Some(c) = char::from_u32(b as u32) {
                    filename.push(c);
                }
            }
            
            if !filename.is_empty() {
                return Some(filename);
            }
        }
    }
    None
}

/// 解析 ZDATA 包，返回数据
pub fn parse_zdata(data: &[u8]) -> Option<Vec<u8>> {
    for i in 0..data.len().saturating_sub(3) {
        if data[i] == ZPAD && 
           data[i+1] == ZPAD && 
           data[i+2] == ZDLE && 
           data[i+3] == ZDATA {
            
            let mut result = Vec::new();
            let mut j = i + 4;
            
            // 跳过 4 字节头部
            let mut header_count = 0;
            while j < data.len() && header_count < 4 {
                if data[j] == ZDLE && j + 1 < data.len() {
                    j += 2;
                    header_count += 1;
                } else {
                    j += 1;
                    header_count += 1;
                }
            }
            
            // 提取数据直到 CRC
            while j < data.len() {
                if data[j] == ZDLE {
                    if j + 2 < data.len() && data[j+1] == ZDLE {
                        break; // CRC 开始
                    }
                    result.push(data[j]);
                    j += 1;
                } else {
                    result.push(data[j]);
                    j += 1;
                }
            }
            
            if !result.is_empty() {
                return Some(result);
            }
        }
    }
    None
}
