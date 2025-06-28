use libc::{c_char, c_double, c_int};
use std::ffi::CStr;
#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
use std::sync::{Mutex, OnceLock};

use crate::system::CokeOvenSystem;

// 全局系统句柄
static SYSTEM: OnceLock<Mutex<Option<CokeOvenSystem>>> = OnceLock::new();

/// 初始化系统（Windows 宽字符版本）
#[cfg(windows)]
#[no_mangle]
pub extern "C" fn coke_system_init_wide(db_path: *const u16) -> c_int {
    let db_path_str = unsafe {
        let mut len = 0;
        let mut ptr = db_path;
        while *ptr != 0 {
            len += 1;
            ptr = ptr.offset(1);
        }
        let slice = std::slice::from_raw_parts(db_path, len);
        OsString::from_wide(slice).to_string_lossy().into_owned()
    };

    init_system(&db_path_str)
}

/// 兼容性包装函数
#[no_mangle]
pub extern "C" fn coke_system_init(db_path: *const c_char) -> c_int {
    let db_path_str = match unsafe { c_char_to_string(db_path) } {
        Ok(s) => s,
        Err(_) => return -1,
    };
    init_system(&db_path_str)
}

/// 记录温度
#[no_mangle]
pub extern "C" fn record_temperature(
    coke_oven: c_int,
    time: *const c_char,
    machine_temp: c_double,
    coke_temp: c_double,
) -> c_int {
    let time_str = match unsafe { c_char_to_string(time) } {
        Ok(s) => s,
        Err(_) => return -2,
    };

    let result = with_system_mut(|system| {
        system.record_temperature(coke_oven as i32, &time_str, machine_temp, coke_temp)
    });

    match result {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            eprintln!("温度记录错误: {}", e);
            -3
        }
        Err(e) => {
            eprintln!("系统错误: {}", e);
            -1
        }
    }
}

/// 记录操作
#[no_mangle]
pub extern "C" fn record_operation(
    coke_oven: c_int,
    chamber: *const c_char,
    op_type: *const c_char,
    time: *const c_char,
) -> c_int {
    let chamber_str = match unsafe { c_char_to_string(chamber) } {
        Ok(s) => s,
        Err(_) => return -2,
    };

    let op_type_str = match unsafe { c_char_to_string(op_type) } {
        Ok(s) => s,
        Err(_) => return -3,
    };

    let time_str = match unsafe { c_char_to_string(time) } {
        Ok(s) => s,
        Err(_) => return -4,
    };

    let result = with_system_mut(|system| {
        system.record_operation(coke_oven as i32, &chamber_str, &op_type_str, &time_str)
    });

    match result {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            eprintln!("操作记录错误: {}", e);
            -5
        }
        Err(e) => {
            eprintln!("系统错误: {}", e);
            -1
        }
    }
}

/// 关闭系统并清理资源
#[no_mangle]
pub extern "C" fn coke_system_shutdown() {
    if let Some(mutex) = SYSTEM.get() {
        let mut guard = mutex.lock().unwrap();
        *guard = None;
    }
}

// ====================== 辅助函数 ======================

unsafe fn c_char_to_string(c_str: *const c_char) -> Result<String, ()> {
    if c_str.is_null() {
        return Err(());
    }
    CStr::from_ptr(c_str)
        .to_str()
        .map(|s| s.to_string())
        .map_err(|_| ())
}

#[no_mangle]
pub unsafe extern "C" fn get_last_error() -> *const c_char {
    static ERROR: &str = "未实现错误跟踪\0";
    ERROR.as_ptr() as *const c_char
}

// 初始化系统通用逻辑
fn init_system(db_path: &str) -> c_int {
    match CokeOvenSystem::new(db_path) {
        Ok(system) => {
            let mutex = SYSTEM.get_or_init(|| Mutex::new(None));
            let mut guard = mutex.lock().unwrap();
            *guard = Some(system);
            0
        }
        Err(e) => {
            eprintln!("初始化错误: {}", e);
            -2
        }
    }
}

// 带错误处理的系统访问
fn with_system_mut<F, T>(f: F) -> Result<Result<T, String>, String>
where
    F: FnOnce(&mut CokeOvenSystem) -> Result<T, String>,
{
    let system = SYSTEM.get().ok_or("系统未初始化".to_string())?;
    let mut guard = system.lock().map_err(|_| "锁获取失败".to_string())?;
    let system = guard.as_mut().ok_or("系统未初始化".to_string())?;
    Ok(f(system))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_c_char_conversion() {
        let s = "test string";
        let c_str = CString::new(s).unwrap();
        let ptr = c_str.as_ptr();

        let result = unsafe { c_char_to_string(ptr) };
        assert_eq!(result, Ok(s.to_string()));

        let null_result = unsafe { c_char_to_string(std::ptr::null()) };
        assert!(null_result.is_err());
    }
}
