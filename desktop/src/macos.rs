use std::path::{Path, PathBuf};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSWorkspace};
use objc2_foundation::{
    NSData, NSString, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
};

use crate::commands::ResolveBookmarkError;

// RAII guard: starts security-scoped access on construction, stops on drop.
pub struct ResolvedUrl {
    url: Retained<NSURL>,
    path: PathBuf,
}

impl ResolvedUrl {
    fn new(url: Retained<NSURL>) -> Option<Self> {
        // Safety: startAccessingSecurityScopedResource / stopAccessingSecurityScopedResource
        // are documented to be safe for app-scoped bookmarks when called from the owning app.
        let started = unsafe { url.startAccessingSecurityScopedResource() };
        if started {
            let ns_path = url.path().expect("security-scoped URL has no path");
            let path = PathBuf::from(ns_path.to_string());
            Some(Self { url, path })
        } else {
            None
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ResolvedUrl {
    fn drop(&mut self) {
        unsafe { self.url.stopAccessingSecurityScopedResource() }
    }
}

// Safety: NSURL is Send + Sync (objc2 provides those impls).
unsafe impl Send for ResolvedUrl {}
unsafe impl Sync for ResolvedUrl {}

/// Must be called on the main thread.
pub fn show_open_panel_sync(preselect: &Path) -> Option<PathBuf> {
    let mtm = MainThreadMarker::new()
        .expect("show_open_panel_sync must be called on the main thread");

    let panel = NSOpenPanel::openPanel(mtm);

    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);

    let message =
        NSString::from_str("Authorize MosaicFS to open files from this mountpoint.");
    panel.setMessage(Some(&message));

    let preselect_str = preselect.to_string_lossy();
    let ns_preselect = NSString::from_str(&preselect_str);
    let dir_url = NSURL::fileURLWithPath(&ns_preselect);
    panel.setDirectoryURL(Some(&dir_url));

    let response = panel.runModal();
    if response != NSModalResponseOK {
        return None;
    }

    let urls = panel.URLs();
    let first = urls.firstObject()?;
    let ns_path = first.path()?;
    Some(PathBuf::from(ns_path.to_string()))
}

pub fn create_bookmark(path: &Path) -> Result<Vec<u8>, String> {
    let path_str = path.to_string_lossy();
    let ns_path = NSString::from_str(&path_str);
    let url = NSURL::fileURLWithPath(&ns_path);
    let options = NSURLBookmarkCreationOptions::WithSecurityScope;
    let data = url
        .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            options, None, None,
        )
        .map_err(|e| e.localizedDescription().to_string())?;
    Ok(data.to_vec())
}

pub fn resolve_bookmark(data: &[u8]) -> Result<ResolvedUrl, ResolveBookmarkError> {
    let ns_data = NSData::with_bytes(data);
    let options = NSURLBookmarkResolutionOptions::WithSecurityScope
        | NSURLBookmarkResolutionOptions::WithoutUI;

    let mut is_stale = Bool::NO;
    // Safety: is_stale is a valid non-null pointer to a Bool on the stack.
    let url = unsafe {
        NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            &ns_data,
            options,
            None,
            &mut is_stale,
        )
        .map_err(|e| ResolveBookmarkError::Other(e.localizedDescription().to_string()))?
    };

    if is_stale.as_bool() {
        return Err(ResolveBookmarkError::Stale);
    }

    ResolvedUrl::new(url).ok_or_else(|| {
        ResolveBookmarkError::Other("failed to start security-scoped resource access".into())
    })
}

pub fn nsworkspace_open(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let ns_path = NSString::from_str(&path_str);
    let url = NSURL::fileURLWithPath(&ns_path);
    let workspace = NSWorkspace::sharedWorkspace();
    workspace.openURL(&url)
}
