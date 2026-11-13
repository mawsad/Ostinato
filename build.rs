use std::fs::File;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let ico_path = Path::new(&out_dir).join("ostinato.ico");

    let w: u32 = 64;
    let h: u32 = 64;
    let mut bmp_data: Vec<u8> = Vec::new();

    bmp_data.extend_from_slice(&40u32.to_le_bytes());
    bmp_data.extend_from_slice(&(w as i32).to_le_bytes());
    bmp_data.extend_from_slice(&((h * 2) as i32).to_le_bytes());
    bmp_data.extend_from_slice(&1u16.to_le_bytes());
    bmp_data.extend_from_slice(&32u16.to_le_bytes());
    bmp_data.extend(std::iter::repeat(0u8).take(24));

    for y in (0..h).rev() {
        for x in 0..w {
            let dx = x as f32 - 31.5;
            let dy = y as f32 - 31.5;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= 30.0 {
                if (dist - 21.0).abs() <= 4.2 || dist <= 6.5 {
                    bmp_data.extend_from_slice(&0xFF0091FFu32.to_le_bytes());
                } else {
                    bmp_data.extend_from_slice(&0xFF050910u32.to_le_bytes());
                }
            } else {
                bmp_data.extend_from_slice(&0u32.to_le_bytes());
            }
        }
    }

    let mask_len = ((w * h) / 8) as usize;
    bmp_data.extend(std::iter::repeat(0u8).take(mask_len));

    let mut ico = File::create(&ico_path).unwrap();
    ico.write_all(&0u16.to_le_bytes()).unwrap();
    ico.write_all(&1u16.to_le_bytes()).unwrap();
    ico.write_all(&1u16.to_le_bytes()).unwrap();

    ico.write_all(&64u8.to_le_bytes()).unwrap();
    ico.write_all(&64u8.to_le_bytes()).unwrap();
    ico.write_all(&0u8.to_le_bytes()).unwrap();
    ico.write_all(&0u8.to_le_bytes()).unwrap();
    ico.write_all(&1u16.to_le_bytes()).unwrap();
    ico.write_all(&32u16.to_le_bytes()).unwrap();
    ico.write_all(&(bmp_data.len() as u32).to_le_bytes()).unwrap();
    ico.write_all(&22u32.to_le_bytes()).unwrap();
    ico.write_all(&bmp_data).unwrap();

    let mut res = winres::WindowsResource::new();
    res.set_icon(ico_path.to_str().unwrap());
    res.set("ProductName", "Ostinato Audio Player");
    res.set("FileDescription", "Ostinato Lightweight Music Client");
    res.set("OriginalFilename", "Ostinato.exe");
    res.set("CompanyName", "Ostinato Open Source");
    res.set("LegalCopyright", "Copyright (c) 2026");
    let _ = res.compile();
}
