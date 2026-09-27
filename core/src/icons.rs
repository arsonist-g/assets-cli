//! 平台图标：本地存放 + 从网址获取。
//!
//! 图标是显示层的附属物，**不进台账**（`data.json`）：一个平台一个文件，落在
//! `<数据目录>/icons/<平台名>.png`。文件名就是平台名，于是台账改名 / 删除时图标跟着走
//! —— `ops::platform_rename` / `ops::platform_delete` 会调这里。
//!
//! 写入时一律解码并转成 PNG（最长边压到 256），界面侧因此只需要认一种格式。

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::paths::Paths;

/// 图标目录名（数据目录下）。
pub const ICON_DIR_NAME: &str = "icons";
/// 单个图标的大小上限（来源文件与落盘后都按它卡）。
pub const ICON_MAX_BYTES: usize = 512 * 1024;
/// 落盘前的最长边上限（像素）。
pub const ICON_MAX_EDGE: u32 = 256;
/// 取图标时的网络超时（毫秒）。
const FETCH_TIMEOUT_MS: u32 = 8000;
/// 一次读取的分块大小。
const READ_CHUNK: usize = 16 * 1024;

/// 解码后的图标像素（RGBA8，行优先）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconPixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 图标目录。
pub fn dir(paths: &Paths) -> PathBuf {
    paths.data_dir.join(ICON_DIR_NAME)
}

/// 某个平台的图标文件路径（文件可能还不存在）。
pub fn path(paths: &Paths, platform: &str) -> Result<PathBuf> {
    Ok(dir(paths).join(format!("{}.png", safe_stem(platform)?)))
}

/// 读图标；没有图标返回 `None`。
pub fn load_pixels(paths: &Paths, platform: &str) -> Result<Option<IconPixels>> {
    let file = path(paths, platform)?;
    match fs::read(&file) {
        Ok(bytes) => Ok(Some(decode_rgba(&bytes)?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(CoreError::storage(format!(
            "图标读不出来 {}：{err}",
            file.display()
        ))),
    }
}

/// 存图标：常见位图（PNG / JPEG / ICO / BMP / GIF / WebP）都收，统一转成 PNG 落盘。
pub fn save(paths: &Paths, platform: &str, bytes: &[u8]) -> Result<PathBuf> {
    if bytes.is_empty() {
        return Err(CoreError::validation("图标内容为空"));
    }
    if bytes.len() > ICON_MAX_BYTES {
        return Err(CoreError::validation(format!(
            "图标太大：{} KiB，上限 {} KiB",
            bytes.len() / 1024,
            ICON_MAX_BYTES / 1024
        )));
    }
    let image = image::load_from_memory(bytes)
        .map_err(|err| CoreError::validation(format!("认不出这个图片格式：{err}")))?;
    let image = if image.width().max(image.height()) > ICON_MAX_EDGE {
        image.resize(
            ICON_MAX_EDGE,
            ICON_MAX_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        image
    };

    let dir = dir(paths);
    fs::create_dir_all(&dir)
        .map_err(|err| CoreError::storage(format!("图标目录建不出来 {}：{err}", dir.display())))?;
    let target = path(paths, platform)?;
    let tmp = target.with_extension("png.tmp");
    image
        .to_rgba8()
        .save_with_format(&tmp, image::ImageFormat::Png)
        .map_err(|err| CoreError::storage(format!("图标写不进去 {}：{err}", tmp.display())))?;
    fs::rename(&tmp, &target)
        .map_err(|err| CoreError::storage(format!("图标落位失败 {}：{err}", target.display())))?;
    Ok(target)
}

/// 从本地文件存图标（大小超限先挡掉，不读进内存）。
pub fn save_from_file(paths: &Paths, platform: &str, src: &Path) -> Result<PathBuf> {
    let meta = fs::metadata(src)
        .map_err(|err| CoreError::usage(format!("读不到这个文件 {}：{err}", src.display())))?;
    if !meta.is_file() {
        return Err(CoreError::usage(format!("不是一个文件：{}", src.display())));
    }
    if meta.len() as usize > ICON_MAX_BYTES {
        return Err(CoreError::validation(format!(
            "图标太大：{} KiB，上限 {} KiB",
            meta.len() / 1024,
            ICON_MAX_BYTES / 1024
        )));
    }
    let bytes = fs::read(src)
        .map_err(|err| CoreError::storage(format!("读不到这个文件 {}：{err}", src.display())))?;
    save(paths, platform, &bytes)
}

/// 删图标；本来就没有也算成功。
pub fn remove(paths: &Paths, platform: &str) -> Result<bool> {
    let target = path(paths, platform)?;
    match fs::remove_file(&target) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(CoreError::storage(format!(
            "图标删不掉 {}：{err}",
            target.display()
        ))),
    }
}

/// 平台改名时把图标一起改名；没有图标就什么都不做。
pub fn rename(paths: &Paths, old: &str, new: &str) -> Result<()> {
    let from = path(paths, old)?;
    let to = path(paths, new)?;
    if !from.exists() {
        return Ok(());
    }
    if to.exists() {
        fs::remove_file(&to)
            .map_err(|err| CoreError::storage(format!("图标删不掉 {}：{err}", to.display())))?;
    }
    fs::rename(&from, &to).map_err(|err| {
        CoreError::storage(format!(
            "图标改名失败 {} → {}：{err}",
            from.display(),
            to.display()
        ))
    })
}

/// 从网址取一份图片字节（HTTP / HTTPS）。
///
/// 走 WinHTTP：它读系统代理设置，本机那种"系统级代理"不需要再配一遍；
/// 也不引入新的三方 HTTP 依赖（本仓其余部分同样只用 Windows 自带的 API）。
pub fn fetch(url: &str) -> Result<Vec<u8>> {
    let target = Url::parse(url)?;
    fetch_winhttp(&target)
}

fn decode_rgba(bytes: &[u8]) -> Result<IconPixels> {
    let image = image::load_from_memory(bytes)
        .map_err(|err| CoreError::validation(format!("图标解不开：{err}")))?;
    let rgba = image.to_rgba8();
    Ok(IconPixels {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// 平台名当文件名用：只收 ASCII 字母 / 数字 / 下划线 / 连字符，避免路径穿越。
fn safe_stem(platform: &str) -> Result<String> {
    let trimmed = platform.trim();
    if trimmed.is_empty() {
        return Err(CoreError::usage("平台名为空，取不到图标路径"));
    }
    let ok = trimmed.len() <= 64
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !ok {
        return Err(CoreError::usage(format!("平台名不能当文件名用：{trimmed}")));
    }
    Ok(trimmed.to_string())
}

/// 拆 URL：只支持 http / https，够图标这一件事用。
struct Url {
    secure: bool,
    host: String,
    port: u16,
    path: String,
}

impl Url {
    fn parse(raw: &str) -> Result<Url> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(CoreError::usage("网址为空"));
        }
        let (scheme, rest) = raw
            .split_once("://")
            .ok_or_else(|| CoreError::usage(format!("网址要带 http:// 或 https://：{raw}")))?;
        let secure = match scheme.to_ascii_lowercase().as_str() {
            "http" => false,
            "https" => true,
            other => {
                return Err(CoreError::usage(format!(
                    "只支持 http / https，不支持：{other}"
                )));
            }
        };
        let (authority, path) = match rest.find(['/', '?']) {
            Some(idx) => (&rest[..idx], rest[idx..].to_string()),
            None => (rest, "/".to_string()),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => {
                let port = port
                    .parse::<u16>()
                    .map_err(|_| CoreError::usage(format!("端口不合法：{port}")))?;
                (host, port)
            }
            _ => (authority, if secure { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err(CoreError::usage(format!("网址里没有主机名：{raw}")));
        }
        Ok(Url {
            secure,
            host: host.to_string(),
            port,
            path,
        })
    }
}

/// 把 WinHTTP 的错误码译成一句人看得懂的话。
///
/// 直接把 `std::io::Error` 摆到界面上，出来的是「OS Error 12029 (FormatMessageW()
/// returned error 317) (os error 12029)」—— 系统消息表里没有 WinHTTP 的条目，Rust 只能
/// 原样报号。常见几种给一句人话，号码留在括号里，方便拿去搜。
#[cfg(windows)]
fn winhttp_error(context: &str) -> CoreError {
    let code = std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or_default();
    let reason = match code {
        12002 => "等超时了，对方没回应",
        12007 => "域名解析不了，检查主机名有没有拼错",
        12029 => "连不上对方，检查主机名和端口",
        12030 | 12031 => "连接中途断了",
        12157 | 12169 | 12175 => "HTTPS 证书没通过校验",
        12005 | 12006 => "网址写法不对",
        _ => "取不到",
    };
    CoreError::storage(format!("{context}：{reason}（{code}）"))
}

#[cfg(windows)]
fn fetch_winhttp(target: &Url) -> Result<Vec<u8>> {
    use std::ffi::c_void;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
        WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
        INTERNET_DEFAULT_HTTPS_PORT, INTERNET_DEFAULT_HTTP_PORT,
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
        WINHTTP_FLAG_SECURE, WINHTTP_OPTION_CONNECT_TIMEOUT, WINHTTP_OPTION_RECEIVE_TIMEOUT,
        WINHTTP_OPTION_SEND_TIMEOUT, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    };

    /// 句柄守卫：无论从哪条路径返回都关掉。
    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }

    let wide = |s: &str| -> Vec<u16> {
        let mut v: Vec<u16> = s.encode_utf16().collect();
        v.push(0);
        v
    };
    let timeout = |handle: *mut c_void, option: u32, ms: u32| unsafe {
        WinHttpSetOption(
            handle,
            option,
            &ms as *const u32 as *const c_void,
            std::mem::size_of::<u32>() as u32,
        );
    };

    let agent = wide("assets-cli");
    let mut session = unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    if session.is_null() {
        session = unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
    }
    if session.is_null() {
        return Err(winhttp_error("打不开 HTTP 会话"));
    }
    let session = Handle(session);
    timeout(session.0, WINHTTP_OPTION_CONNECT_TIMEOUT, FETCH_TIMEOUT_MS);
    timeout(session.0, WINHTTP_OPTION_SEND_TIMEOUT, FETCH_TIMEOUT_MS);
    timeout(session.0, WINHTTP_OPTION_RECEIVE_TIMEOUT, FETCH_TIMEOUT_MS);

    let host = wide(&target.host);
    let port = if target.port != 0 {
        target.port
    } else if target.secure {
        INTERNET_DEFAULT_HTTPS_PORT
    } else {
        INTERNET_DEFAULT_HTTP_PORT
    };
    let connect = unsafe { WinHttpConnect(session.0, host.as_ptr(), port, 0) };
    if connect.is_null() {
        return Err(winhttp_error(&format!("连不上 {}", target.host)));
    }
    let connect = Handle(connect);

    let verb = wide("GET");
    let object = wide(&target.path);
    let flags = if target.secure {
        WINHTTP_FLAG_SECURE
    } else {
        0
    };
    let request = unsafe {
        WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            object.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            flags,
        )
    };
    if request.is_null() {
        return Err(winhttp_error("构造请求失败"));
    }
    let request = Handle(request);

    let sent =
        unsafe { WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) };
    if sent == 0 {
        return Err(winhttp_error("请求发不出去"));
    }
    let received = unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) };
    if received == 0 {
        return Err(winhttp_error("等不到响应"));
    }

    let mut status: u32 = 0;
    let mut status_len = std::mem::size_of::<u32>() as u32;
    let queried = unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut status_len,
            std::ptr::null_mut(),
        )
    };
    if queried == 0 {
        return Err(winhttp_error("读不到状态码"));
    }
    if status != 200 {
        return Err(CoreError::storage(format!("下载失败：HTTP {status}")));
    }

    let mut out: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        let mut read: u32 = 0;
        let ok = unsafe {
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut c_void,
                chunk.len() as u32,
                &mut read,
            )
        };
        if ok == 0 {
            return Err(winhttp_error("读响应失败"));
        }
        if read == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..read as usize]);
        if out.len() > ICON_MAX_BYTES {
            return Err(CoreError::validation(format!(
                "图标太大：超过 {} KiB",
                ICON_MAX_BYTES / 1024
            )));
        }
    }
    if out.is_empty() {
        return Err(CoreError::storage("对方返回了空内容"));
    }
    Ok(out)
}

#[cfg(not(windows))]
fn fetch_winhttp(_target: &Url) -> Result<Vec<u8>> {
    Err(CoreError::storage("取图标目前只支持 Windows".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn scratch(name: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("assets-icons-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Paths::explicit(&dir, r"HKCU\Environment").unwrap()
    }

    /// 生成一张最普通的 PNG：一张纯色小图，用它验证"解码 → 落盘 → 回读"。
    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let rgba =
            image::RgbaImage::from_pixel(width, height, image::Rgba([0x02, 0x78, 0x7d, 0xff]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn save_load_rename_remove() {
        let paths = scratch("roundtrip");
        assert!(load_pixels(&paths, "CLOUDFLARE").unwrap().is_none());

        let file = save(&paths, "CLOUDFLARE", &png_bytes(16, 16)).unwrap();
        assert!(file.exists());
        let pixels = load_pixels(&paths, "CLOUDFLARE").unwrap().unwrap();
        assert_eq!((pixels.width, pixels.height), (16, 16));
        assert_eq!(pixels.rgba.len(), 16 * 16 * 4);

        // 大图会被压到上限内。
        save(&paths, "CLOUDFLARE", &png_bytes(512, 256)).unwrap();
        let pixels = load_pixels(&paths, "CLOUDFLARE").unwrap().unwrap();
        assert_eq!(pixels.width.max(pixels.height), ICON_MAX_EDGE);

        rename(&paths, "CLOUDFLARE", "DOCPARSE").unwrap();
        assert!(load_pixels(&paths, "CLOUDFLARE").unwrap().is_none());
        assert!(load_pixels(&paths, "DOCPARSE").unwrap().is_some());

        assert!(remove(&paths, "DOCPARSE").unwrap());
        assert!(!remove(&paths, "DOCPARSE").unwrap());
        assert!(load_pixels(&paths, "DOCPARSE").unwrap().is_none());
    }

    #[test]
    fn rejects_bad_input() {
        let paths = scratch("rejects");
        assert!(save(&paths, "CLOUDFLARE", b"").is_err());
        assert!(save(&paths, "CLOUDFLARE", b"not an image").is_err());
        assert!(path(&paths, "../escape").is_err());
        assert!(path(&paths, "").is_err());
        assert!(Url::parse("ftp://example.com/favicon.ico").is_err());
        assert!(Url::parse("example.com/favicon.ico").is_err());
        let https = Url::parse("https://example.com/a/b.png").unwrap();
        assert!(https.secure);
        assert_eq!(https.port, 443);
        assert_eq!(https.path, "/a/b.png");
        let plain = Url::parse("http://example.com:8080").unwrap();
        assert!(!plain.secure);
        assert_eq!(plain.port, 8080);
        assert_eq!(plain.path, "/");
    }
}
