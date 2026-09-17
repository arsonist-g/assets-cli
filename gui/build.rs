fn main() {
    slint_build::compile("ui/main.slint").expect("编译 ui/main.slint 失败");
    embed_windows_resources();
}

/// 把应用图标与文件描述写进 PE 资源段。
/// 资源管理器、任务栏与快捷方式读的都是 PE 资源，Slint 本身不提供设置窗口图标的接口。
/// rc.exe 来自 Windows SDK：找不到就只发警告不报错，缺图标不影响程序运行。
#[cfg(windows)]
fn embed_windows_resources() {
    const ICON: &str = "assets/assets-gui.ico";
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=build.rs");

    if !std::path::Path::new(ICON).exists() {
        println!("cargo:warning=没有找到 {ICON}，跳过图标与文件描述嵌入");
        return;
    }

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ICON);
    res.set("ProductName", "assets");
    res.set("FileDescription", "assets 资产台账");
    res.set("OriginalFilename", "assets-gui.exe");
    res.set_manifest(&manifest(&version));
    if let Err(err) = res.compile() {
        println!("cargo:warning=嵌入 Windows 资源失败：{err}（只影响图标与文件属性）");
    }
}

#[cfg(not(windows))]
fn embed_windows_resources() {}

/// 程序清单：声明 PerMonitorV2 DPI 感知。没有它，系统会按 96 DPI 渲染再整体拉伸，高分屏上发虚。
fn manifest(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="assets-gui" version="{version}.0" processorArchitecture="*"/>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#
    )
}
