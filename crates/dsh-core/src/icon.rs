//! ICO 图标解析（纯逻辑，无 GUI 依赖，可单测）。
//!
//! 为什么需要它：tao 创建窗口时若不显式设置图标，Windows 会用窗口类的默认图标，
//! 表现为标题栏左上角**不显示应用 logo**。这里把内嵌的 `app.ico` 解析为 RGBA，
//! 供 `tao::window::Icon::from_rgba` 使用。
//!
//! 支持的条目格式：**32bpp 未压缩 DIB**（BITMAPINFOHEADER + 自下而上的 BGRA 行）。
//! PNG 编码的条目（通常只有 256×256）会被跳过——选取尺寸时避开即可。

/// 解析出的位图。
#[derive(Debug, Clone, PartialEq)]
pub struct IconImage {
    pub width: u32,
    pub height: u32,
    /// RGBA8，行优先、自上而下，长度 = width * height * 4
    pub rgba: Vec<u8>,
}

const ICONDIR_SIZE: usize = 6;
const ICONDIRENTRY_SIZE: usize = 16;

/// 从 ICO 字节中解析图像，优先返回尺寸最接近 `preferred` 的条目。
///
/// 选择策略：恰好等于 `preferred` > 大于 `preferred` 中最小的 > 最大的可用条目。
pub fn parse_ico(data: &[u8], preferred: u32) -> Option<IconImage> {
    let count = read_u16(data, 4)? as usize;
    if count == 0 || data.len() < ICONDIR_SIZE + count * ICONDIRENTRY_SIZE {
        return None;
    }

    // 收集全部候选条目：(实际尺寸, 偏移, 字节数)
    let mut entries: Vec<(u32, usize, usize)> = Vec::new();
    for i in 0..count {
        let base = ICONDIR_SIZE + i * ICONDIRENTRY_SIZE;
        // bWidth/bHeight 为 0 表示 256
        let w = match data.get(base)? {
            0 => 256u32,
            v => *v as u32,
        };
        let h = match data.get(base + 1)? {
            0 => 256u32,
            v => *v as u32,
        };
        let bytes = read_u32(data, base + 8)? as usize;
        let offset = read_u32(data, base + 12)? as usize;
        if w != h {
            continue; // 非正方形条目跳过，避免选到异常图标
        }
        if offset.checked_add(bytes)? > data.len() {
            continue;
        }
        entries.push((w, offset, bytes));
    }
    if entries.is_empty() {
        return None;
    }

    // 按选择策略排序候选
    entries.sort_by_key(|(size, _, _)| {
        if *size == preferred {
            (0u8, 0u32)
        } else if *size > preferred {
            (1, *size)
        } else {
            (2, u32::MAX - *size) // 小于 preferred 的里取最大的
        }
    });

    for (size, offset, bytes) in entries {
        let blob = &data[offset..offset + bytes];
        if let Some(img) = parse_dib_entry(blob, size) {
            return Some(img);
        }
        // PNG 条目：无解码器，跳过并尝试下一个候选
    }
    None
}

/// 解析一个 32bpp 未压缩 DIB 条目（BITMAPINFOHEADER 起）。
fn parse_dib_entry(blob: &[u8], expected_size: u32) -> Option<IconImage> {
    let header_size = read_u32(blob, 0)? as usize;
    if header_size < 40 || blob.len() < header_size {
        return None;
    }
    let width = read_i32(blob, 4)?;
    let height2 = read_i32(blob, 8)?; // 含 AND 掩码，故为实际高度的两倍
    let bit_count = read_u16(blob, 14)?;
    let compression = read_u32(blob, 16)?;

    // 只处理 32bpp 未压缩（带 alpha 通道，最可靠）
    if bit_count != 32 || compression != 0 || width <= 0 || height2 <= 0 {
        return None;
    }
    let w = width as u32;
    let h = (height2 as u32) / 2;
    if h == 0 || w != expected_size || h != expected_size {
        return None;
    }

    let stride = (w * 4) as usize;
    let need = header_size + stride * h as usize;
    if blob.len() < need {
        return None;
    }

    // DIB 行序自下而上，转为自上而下的 RGBA
    let mut rgba = vec![0u8; stride * h as usize];
    for y in 0..h as usize {
        let src = header_size + (h as usize - 1 - y) * stride;
        let dst = y * stride;
        for x in 0..w as usize {
            let s = src + x * 4;
            let d = dst + x * 4;
            // DIB 存放顺序为 BGRA，输出需要 RGBA
            rgba[d] = blob[s + 2]; // R
            rgba[d + 1] = blob[s + 1]; // G
            rgba[d + 2] = blob[s]; // B
            rgba[d + 3] = blob[s + 3]; // A
        }
    }

    Some(IconImage {
        width: w,
        height: h,
        rgba,
    })
}

fn read_u16(d: &[u8], off: usize) -> Option<u16> {
    let b = d.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(d: &[u8], off: usize) -> Option<u32> {
    let b = d.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_i32(d: &[u8], off: usize) -> Option<i32> {
    read_u32(d, off).map(|v| v as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个最小的 32bpp ICO：2 个条目（16×16 与 32×32），均为 DIB。
    fn build_test_ico() -> Vec<u8> {
        let sizes = [16u32, 32u32];
        let mut blobs: Vec<Vec<u8>> = Vec::new();
        for (idx, sz) in sizes.iter().enumerate() {
            let stride = (*sz * 4) as usize;
            let mut b = vec![0u8; 40 + stride * (*sz as usize)];
            b[0..4].copy_from_slice(&40u32.to_le_bytes());
            b[4..8].copy_from_slice(&(*sz as i32).to_le_bytes());
            b[8..12].copy_from_slice(&((*sz * 2) as i32).to_le_bytes());
            b[12..14].copy_from_slice(&1u16.to_le_bytes());
            b[14..16].copy_from_slice(&32u16.to_le_bytes());
            // 像素：B 通道填 idx+1，便于区分选中了哪一条（DIB 的 B → RGBA 第 3 字节）
            for y in 0..*sz as usize {
                for x in 0..*sz as usize {
                    let p = 40 + y * stride + x * 4;
                    b[p] = (idx as u8) + 1; // B
                    b[p + 1] = 10; // G
                    b[p + 2] = 20; // R
                    b[p + 3] = 255; // A
                }
            }
            blobs.push(b);
        }

        let count = blobs.len();
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_le_bytes()); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // type = icon
        out.extend_from_slice(&(count as u16).to_le_bytes());

        let mut offset = ICONDIR_SIZE + count * ICONDIRENTRY_SIZE;
        for (i, b) in blobs.iter().enumerate() {
            let sz = sizes[i] as u8;
            out.push(sz); // width
            out.push(sz); // height
            out.push(0); // colors
            out.push(0); // reserved
            out.extend_from_slice(&1u16.to_le_bytes()); // planes
            out.extend_from_slice(&32u16.to_le_bytes()); // bitcount
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += b.len();
        }
        for b in &blobs {
            out.extend_from_slice(b);
        }
        out
    }

    #[test]
    fn parses_exact_preferred_size() {
        let ico = build_test_ico();
        let img = parse_ico(&ico, 32).expect("应解析成功");
        assert_eq!((img.width, img.height), (32, 32));
        assert_eq!(img.rgba.len(), 32 * 32 * 4);
        // 32×32 是第 2 个条目(idx=1)，其 B 通道 = 2 → RGBA 第 3 字节
        assert_eq!(img.rgba[2], 2, "应选中 32×32 条目");
        assert_eq!(img.rgba[0], 20, "R 通道应正确映射");
        assert_eq!(img.rgba[3], 255, "alpha 应保留");
    }

    #[test]
    fn falls_back_to_smaller_when_preferred_missing() {
        let ico = build_test_ico();
        // 请求 48：不存在，应回退到最大的较小条目 32
        let img = parse_ico(&ico, 48).expect("应回退解析成功");
        assert_eq!(img.width, 32);
    }

    #[test]
    fn picks_smallest_larger_when_available() {
        let ico = build_test_ico();
        // 请求 16：存在精确匹配
        let img = parse_ico(&ico, 16).unwrap();
        assert_eq!(img.width, 16);
        // 请求 8：应取大于 8 中最小的 = 16
        let img2 = parse_ico(&ico, 8).unwrap();
        assert_eq!(img2.width, 16);
    }

    #[test]
    fn handles_truncated_and_garbage_input() {
        assert!(parse_ico(&[], 32).is_none());
        assert!(parse_ico(&[0, 0, 1, 0], 32).is_none());
        assert!(parse_ico(&[0u8; 64], 32).is_none());
        // 声明有 2 个条目但数据不足
        let mut bad = vec![0u8, 0, 1, 0, 2, 0];
        bad.extend_from_slice(&[0u8; 10]);
        assert!(parse_ico(&bad, 32).is_none());
    }

    #[test]
    fn rejects_out_of_range_offset() {
        // 条目声明的偏移超出数据范围 → 该条目被跳过，整体返回 None
        let mut ico = build_test_ico();
        // 把第 1 个条目的 imageOffset 改成极大的值
        ico[ICONDIR_SIZE + 12..ICONDIR_SIZE + 16].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
        // 第 2 个条目仍然有效，因此应能解析出 32×32
        let img = parse_ico(&ico, 32);
        assert!(img.is_some());
    }
}
