//! Theme-aware brand icon; independent of the live quota tray indicator.
const DARK: &[u8] = include_bytes!("../icons/icon.png");
const LIGHT: &[u8] = include_bytes!("../icons/light/icon.png");

fn icon_bytes(theme: &str) -> Result<&'static [u8], String> {
    match theme {
        "dark" => Ok(DARK),
        "light" => Ok(LIGHT),
        _ => Err("invalid_icon_theme".into()),
    }
}

#[cfg(target_os = "macos")]
fn apply(app: &tauri::AppHandle, bytes: &[u8]) -> Result<(), String> {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let _ = app;
    let mtm = MainThreadMarker::new().ok_or("icon_main_thread_required")?;
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))
        .ok_or("invalid_app_icon")?;
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn apply(app: &tauri::AppHandle, bytes: &[u8]) -> Result<(), String> {
    use tauri::Manager;
    let image = tauri::image::Image::from_bytes(bytes).map_err(|_| "invalid_app_icon")?;
    for window in app.webview_windows().values() {
        window
            .set_icon(image.clone())
            .map_err(|_| "icon_update_failed")?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_app_icon_theme(app: tauri::AppHandle, theme: String) -> Result<(), String> {
    let bytes = icon_bytes(&theme)?;
    let handle = app.clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = send.send(apply(&handle, bytes));
    })
    .map_err(|_| "icon_update_failed")?;
    receive.await.map_err(|_| "icon_update_failed")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_themes_have_matching_high_dpi_dimensions() {
        for theme in ["light", "dark"] {
            let image = tauri::image::Image::from_bytes(icon_bytes(theme).unwrap()).unwrap();
            assert_eq!((image.width(), image.height()), (512, 512));
        }
        assert!(icon_bytes("system").is_err());
        assert_ne!(DARK, LIGHT);
    }

    #[test]
    fn rounded_tiles_have_transparent_corners_and_an_opaque_center() {
        let sources: [(&str, &[u8]); 4] = [
            ("dark source", include_bytes!("../app-icon-dark.png")),
            ("light source", include_bytes!("../app-icon-light.png")),
            ("dark desktop", DARK),
            ("light desktop", LIGHT),
        ];
        for (name, bytes) in sources {
            let image = tauri::image::Image::from_bytes(bytes).unwrap();
            let width = image.width() as usize;
            let height = image.height() as usize;
            let alpha = |x: usize, y: usize| image.rgba()[(y * width + x) * 4 + 3];
            for (x, y) in [
                (0, 0),
                (width - 1, 0),
                (0, height - 1),
                (width - 1, height - 1),
                (width / 2, 0),
                (width / 2, height - 1),
                (0, height / 2),
                (width - 1, height / 2),
            ] {
                assert_eq!(
                    alpha(x, y),
                    0,
                    "{name}: outside the tile must be transparent"
                );
            }
            assert_eq!(alpha(width / 2, height / 2), 255, "{name}: opaque artwork");
            let transparent = image
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|rgba| rgba[3] == 0)
                .count();
            assert!(
                transparent > width * height / 20,
                "{name}: rounded tile margin"
            );
            assert!(
                image
                    .rgba()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|rgba| rgba[3] > 0 && rgba[3] < 255),
                "{name}: antialiased edge"
            );
        }
    }
}
