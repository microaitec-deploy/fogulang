# WebView2Loader (Windows, GNU toolchain)

`x64/WebView2Loader.dll` is Microsoft's WebView2 loader, from the WebView2 SDK
(the copy that `webview2-com-sys` ships; redistributable under the SDK license).

The MSVC toolchain links the loader statically. The GNU toolchain (our
cross-compiled Windows hosts) can only import the DLL, and an executable that
imports it does not start without `WebView2Loader.dll` next to it
(`0xC0000135`, "WebView2Loader.dll was not found"). So, for `*-windows-gnu`:

* `build.rs` turns `WebView2Loader.def` into a **delay-load** import library
  (`dlltool --output-delaylib`) that the linker finds before the plain one: the
  DLL is loaded on the first WebView2 call instead of at process start;
* `src/main.rs` embeds the DLL and, before Tauri starts, loads the copy next to
  the executable or, when there is none, an extracted copy in
  `%LOCALAPPDATA%\fogu\desktop\webview2\<hash>\`.

The result is one `.exe` that runs on its own.
