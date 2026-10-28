use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use objc2_foundation::NSFileManager;
use objc2_foundation::NSString;
use objc2_foundation::NSURL;

use crate::Error;
use crate::TrashItem;
use crate::restore_from;

/// Trashes with `NSFileManager`, which needs no extra permissions and returns
/// where the item landed. Unlike Finder, Finder's "Put Back" isn't offered for
/// it.
pub(crate) fn delete(path: PathBuf) -> Result<TrashItem, Error> {
    // APFS and HFS+ file names are always valid UTF-8.
    let path_str = path.to_str().ok_or_else(|| Error::Unknown { description: format!("{path:?} is not UTF-8") })?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path_str));
    let mut trashed_url = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut trashed_url))
        .map_err(|error| Error::Unknown { description: format!("`trashItemAtURL` failed for {path:?}: {error}") })?;

    // Without a resulting URL the item is trashed but can't be restored;
    // restoring then reports it as no longer in the trash.
    let id = trashed_url.and_then(|url| url.path()).map(|path| OsString::from(path.to_string())).unwrap_or_default();
    Ok(TrashItem {
        id,
        name: path.file_name().expect("trashed paths are canonical and have a file name").to_owned(),
        original_parent: path.parent().expect("trashed paths are canonical and have a parent").to_owned(),
    })
}

/// Moves the item back by renaming. The trash's `.DS_Store` isn't updated,
/// which Finder tolerates.
pub(crate) fn restore(item: TrashItem) -> Result<(), Error> {
    restore_from(Path::new(&item.id), &item)
}
