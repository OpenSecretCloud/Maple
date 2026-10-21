//! Shared local free-space admission; errors contain no paths.

#[cfg(unix)]
use super::StoreError;
use super::StoreResult;
use std::{io, path::Path};

#[cfg(unix)]
pub(super) fn free_bytes(path: &Path) -> StoreResult<u64> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| StoreError::UnsafePath)?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let stat = unsafe { stat.assume_init() };
    let bytes = u128::from(stat.f_bavail).saturating_mul(u128::from(stat.f_frsize));
    Ok(u64::try_from(bytes).unwrap_or(u64::MAX))
}

#[cfg(windows)]
pub(super) fn free_bytes(path: &Path) -> StoreResult<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    if unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(available)
}
