#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::runtime::*;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;

pub type CFStringRef = CFTypeRef;
pub type CFDictionaryRef = CFTypeRef;
pub type CFMutableDictionaryRef = CFTypeRef;
pub type CFArrayRef = CFTypeRef;
pub type CFBooleanRef = CFTypeRef;
pub type CFAllocatorRef = CFTypeRef;
pub type CFNumberRef = CFTypeRef;
pub type CFDataRef = CFTypeRef;
pub type CFIndex = i64;
pub type Boolean = i8;
pub type CFTypeID = u64;
pub type CFStringEncoding = u32;

#[repr(transparent)]
pub struct ExportedCFRef(pub *const c_void);
unsafe impl Sync for ExportedCFRef {}

pub const kCFStringEncodingUTF8: CFStringEncoding = 0x0800_0100;
pub const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
pub const K_CF_NUMBER_SINT64_TYPE: i32 = 4;
pub const K_CF_NUMBER_DOUBLE_TYPE: i32 = 6;
pub const K_CF_NUMBER_INT_TYPE: i32 = 9;

#[repr(C)]
pub struct CFRange {
    pub location: CFIndex,
    pub length: CFIndex,
}

#[repr(C)]
pub struct CFArrayCallBacks {
    pub version: CFIndex,
    pub retain: *const c_void,
    pub release: *const c_void,
    pub copyDescription: *const c_void,
    pub equal: *const c_void,
}
unsafe impl Sync for CFArrayCallBacks {}

#[repr(C)]
pub struct CFDictionaryKeyCallBacks {
    pub version: CFIndex,
    pub retain: *const c_void,
    pub release: *const c_void,
    pub copyDescription: *const c_void,
    pub equal: *const c_void,
    pub hash: *const c_void,
}
unsafe impl Sync for CFDictionaryKeyCallBacks {}

#[repr(C)]
pub struct CFDictionaryValueCallBacks {
    pub version: CFIndex,
    pub retain: *const c_void,
    pub release: *const c_void,
    pub copyDescription: *const c_void,
    pub equal: *const c_void,
}
unsafe impl Sync for CFDictionaryValueCallBacks {}

#[repr(C)]
pub struct vtf_cf_string {
    pub base: vtf_cf_object,
    pub bytes: *const u8,
    pub length: usize,
    pub owns_bytes: bool,
}

#[repr(C)]
pub struct vtf_cf_number {
    pub base: vtf_cf_object,
    pub number_type: i32,
    pub sint64: i64,
    pub f64: f64,
}

#[repr(C)]
pub struct vtf_cf_data {
    pub base: vtf_cf_object,
    pub bytes: Vec<u8>,
}

#[repr(C)]
pub struct vtf_cf_array {
    pub base: vtf_cf_object,
    pub values: Vec<CFTypeRef>,
}

#[repr(C)]
pub struct vtf_cf_dictionary {
    pub base: vtf_cf_object,
    pub keys: Vec<CFTypeRef>,
    pub values: Vec<CFTypeRef>,
}

#[repr(C)]
pub struct vtf_cf_boolean {
    pub base: vtf_cf_object,
    pub value: bool,
}

unsafe fn vtf_finalize_string(obj: *mut vtf_cf_object) {
    let string_obj = obj as *mut vtf_cf_string;
    if (*string_obj).owns_bytes && !(*string_obj).bytes.is_null() {
        let _ = Box::from_raw(std::slice::from_raw_parts_mut(
            (*string_obj).bytes as *mut u8,
            (*string_obj).length,
        ));
    }
    let _ = Box::from_raw(string_obj);
}

unsafe fn vtf_finalize_number(obj: *mut vtf_cf_object) {
    let _ = Box::from_raw(obj as *mut vtf_cf_number);
}

unsafe fn vtf_finalize_data(obj: *mut vtf_cf_object) {
    let _ = Box::from_raw(obj as *mut vtf_cf_data);
}

unsafe fn vtf_finalize_array(obj: *mut vtf_cf_object) {
    let array = obj as *mut vtf_cf_array;
    for value in &(*array).values {
        crate::runtime::CFRelease(*value);
    }
    let _ = Box::from_raw(array);
}

unsafe fn vtf_finalize_dictionary(obj: *mut vtf_cf_object) {
    let dictionary = obj as *mut vtf_cf_dictionary;
    for key in &(*dictionary).keys {
        crate::runtime::CFRelease(*key);
    }
    for value in &(*dictionary).values {
        crate::runtime::CFRelease(*value);
    }
    let _ = Box::from_raw(dictionary);
}

unsafe fn vtf_string_contents(value: CFStringRef) -> Option<&'static [u8]> {
    if value.is_null() || vtf_get_type_id(value) != VTF_TYPE_STRING {
        return None;
    }
    let string = &*(value as *const vtf_cf_string);
    if string.bytes.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts(string.bytes, string.length))
}

unsafe fn vtf_dictionary_find_index(
    dictionary: *const vtf_cf_dictionary,
    key: *const c_void,
) -> Option<usize> {
    (*dictionary)
        .keys
        .iter()
        .position(|existing| *existing == key)
}

#[no_mangle]
pub static kCFAllocatorDefault: ExportedCFRef = ExportedCFRef(ptr::null());
#[no_mangle]
pub static kCFAllocatorNull: ExportedCFRef = ExportedCFRef(ptr::null());

#[no_mangle]
pub static kCFTypeArrayCallBacks: CFArrayCallBacks = CFArrayCallBacks {
    version: 0,
    retain: ptr::null(),
    release: ptr::null(),
    copyDescription: ptr::null(),
    equal: ptr::null(),
};

#[no_mangle]
pub static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks = CFDictionaryKeyCallBacks {
    version: 0,
    retain: ptr::null(),
    release: ptr::null(),
    copyDescription: ptr::null(),
    equal: ptr::null(),
    hash: ptr::null(),
};

#[no_mangle]
pub static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks =
    CFDictionaryValueCallBacks {
        version: 0,
        retain: ptr::null(),
        release: ptr::null(),
        copyDescription: ptr::null(),
        equal: ptr::null(),
    };

#[no_mangle]
pub static kCFCopyStringDictionaryKeyCallBacks: CFDictionaryKeyCallBacks =
    CFDictionaryKeyCallBacks {
        version: 0,
        retain: ptr::null(),
        release: ptr::null(),
        copyDescription: ptr::null(),
        equal: ptr::null(),
        hash: ptr::null(),
    };

static mut VTF_KCFBOOLEAN_TRUE_STORAGE: vtf_cf_boolean = vtf_cf_boolean {
    base: vtf_cf_object {
        magic: 0x534d5654,
        type_id: VTF_TYPE_BOOLEAN,
        refcount: std::sync::atomic::AtomicI32::new(1),
        flags: VTF_OBJECT_FLAG_STATIC,
        proxy_id: 0,
        generation: 1,
        host_id: 0,
        finalize: None,
    },
    value: true,
};

static mut VTF_KCFBOOLEAN_FALSE_STORAGE: vtf_cf_boolean = vtf_cf_boolean {
    base: vtf_cf_object {
        magic: 0x534d5654,
        type_id: VTF_TYPE_BOOLEAN,
        refcount: std::sync::atomic::AtomicI32::new(1),
        flags: VTF_OBJECT_FLAG_STATIC,
        proxy_id: 0,
        generation: 1,
        host_id: 0,
        finalize: None,
    },
    value: false,
};

#[no_mangle]
pub static kCFBooleanTrue: ExportedCFRef =
    ExportedCFRef(&raw const VTF_KCFBOOLEAN_TRUE_STORAGE as *const _ as *const c_void);
#[no_mangle]
pub static kCFBooleanFalse: ExportedCFRef =
    ExportedCFRef(&raw const VTF_KCFBOOLEAN_FALSE_STORAGE as *const _ as *const c_void);

#[no_mangle]
pub unsafe extern "C" fn VTF_CFStringCreateStatic(bytes: *const c_char) -> CFStringRef {
    if bytes.is_null() {
        return ptr::null();
    }
    let cstr = CStr::from_ptr(bytes);
    Box::into_raw(Box::new(vtf_cf_string {
        base: vtf_cf_object::init(VTF_TYPE_STRING, Some(vtf_finalize_string)),
        bytes: cstr.as_ptr() as *const u8,
        length: cstr.to_bytes().len(),
        owns_bytes: false,
    })) as CFStringRef
}

#[no_mangle]
pub unsafe extern "C" fn CFGetTypeID(value: CFTypeRef) -> CFTypeID {
    vtf_get_type_id(value)
}

#[no_mangle]
pub unsafe extern "C" fn CFStringGetLength(value: CFStringRef) -> CFIndex {
    vtf_string_contents(value)
        .map(|bytes| bytes.len() as CFIndex)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn CFStringGetMaximumSizeForEncoding(
    length: CFIndex,
    _encoding: CFStringEncoding,
) -> CFIndex {
    if length < 0 {
        0
    } else {
        length + 1
    }
}

#[no_mangle]
pub unsafe extern "C" fn CFStringGetCString(
    value: CFStringRef,
    buffer: *mut c_char,
    buffer_size: CFIndex,
    _encoding: CFStringEncoding,
) -> Boolean {
    let Some(bytes) = vtf_string_contents(value) else {
        return 0;
    };
    if buffer.is_null() || buffer_size <= 0 || (buffer_size as usize) <= bytes.len() {
        return 0;
    }
    ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, buffer, bytes.len());
    *buffer.add(bytes.len()) = 0;
    1
}

#[no_mangle]
pub unsafe extern "C" fn CFNumberCreate(
    _allocator: CFAllocatorRef,
    number_type: i32,
    value_ptr: *const c_void,
) -> CFNumberRef {
    if value_ptr.is_null() {
        return ptr::null();
    }
    let (sint64, f64) = match number_type {
        K_CF_NUMBER_SINT32_TYPE | K_CF_NUMBER_INT_TYPE => {
            let value = *(value_ptr as *const i32);
            (value as i64, value as f64)
        }
        K_CF_NUMBER_SINT64_TYPE => {
            let value = *(value_ptr as *const i64);
            (value, value as f64)
        }
        K_CF_NUMBER_DOUBLE_TYPE => {
            let value = *(value_ptr as *const f64);
            (value as i64, value)
        }
        _ => return ptr::null(),
    };

    Box::into_raw(Box::new(vtf_cf_number {
        base: vtf_cf_object::init(VTF_TYPE_NUMBER, Some(vtf_finalize_number)),
        number_type,
        sint64,
        f64,
    })) as CFNumberRef
}

#[no_mangle]
pub extern "C" fn CFNumberGetTypeID() -> CFTypeID {
    VTF_TYPE_NUMBER
}

#[no_mangle]
pub unsafe extern "C" fn CFNumberGetType(value: CFNumberRef) -> i32 {
    if value.is_null() || vtf_get_type_id(value) != VTF_TYPE_NUMBER {
        return 0;
    }
    (*(value as *const vtf_cf_number)).number_type
}

#[no_mangle]
pub unsafe extern "C" fn CFNumberGetValue(
    value: CFNumberRef,
    number_type: i32,
    out_value: *mut c_void,
) -> Boolean {
    if value.is_null() || out_value.is_null() || vtf_get_type_id(value) != VTF_TYPE_NUMBER {
        return 0;
    }
    let number = &*(value as *const vtf_cf_number);
    match number_type {
        K_CF_NUMBER_SINT32_TYPE | K_CF_NUMBER_INT_TYPE => {
            *(out_value as *mut i32) = number.sint64 as i32;
            1
        }
        K_CF_NUMBER_SINT64_TYPE => {
            *(out_value as *mut i64) = number.sint64;
            1
        }
        K_CF_NUMBER_DOUBLE_TYPE => {
            *(out_value as *mut f64) = number.f64;
            1
        }
        _ => 0,
    }
}

#[no_mangle]
pub extern "C" fn CFDataGetTypeID() -> CFTypeID {
    VTF_TYPE_DATA
}

#[no_mangle]
pub unsafe extern "C" fn CFDataCreate(
    _allocator: CFAllocatorRef,
    bytes: *const u8,
    length: CFIndex,
) -> CFDataRef {
    if length < 0 {
        return ptr::null();
    }
    let data = if bytes.is_null() || length == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(bytes, length as usize).to_vec()
    };
    Box::into_raw(Box::new(vtf_cf_data {
        base: vtf_cf_object::init(VTF_TYPE_DATA, Some(vtf_finalize_data)),
        bytes: data,
    })) as CFDataRef
}

#[no_mangle]
pub unsafe extern "C" fn CFDataGetLength(data: CFDataRef) -> CFIndex {
    if data.is_null() || vtf_get_type_id(data) != VTF_TYPE_DATA {
        return 0;
    }
    (*(data as *const vtf_cf_data)).bytes.len() as CFIndex
}

#[no_mangle]
pub unsafe extern "C" fn CFDataGetBytes(data: CFDataRef, range: CFRange, buffer: *mut u8) {
    if data.is_null() || buffer.is_null() || vtf_get_type_id(data) != VTF_TYPE_DATA {
        return;
    }
    let bytes = &(*(data as *const vtf_cf_data)).bytes;
    if range.location < 0 || range.length < 0 {
        return;
    }
    let start = range.location as usize;
    let len = range.length as usize;
    if start > bytes.len() || start + len > bytes.len() {
        return;
    }
    ptr::copy_nonoverlapping(bytes[start..start + len].as_ptr(), buffer, len);
}

#[no_mangle]
pub unsafe extern "C" fn CFArrayCreate(
    _allocator: CFAllocatorRef,
    values: *const *const c_void,
    num_values: CFIndex,
    _callbacks: *const CFArrayCallBacks,
) -> CFArrayRef {
    if num_values < 0 {
        return ptr::null();
    }
    let mut retained = Vec::with_capacity(num_values as usize);
    for index in 0..(num_values as usize) {
        let value = if values.is_null() {
            ptr::null()
        } else {
            *values.add(index)
        };
        retained.push(crate::runtime::CFRetain(value));
    }
    Box::into_raw(Box::new(vtf_cf_array {
        base: vtf_cf_object::init(VTF_TYPE_ARRAY, Some(vtf_finalize_array)),
        values: retained,
    })) as CFArrayRef
}

#[no_mangle]
pub extern "C" fn CFArrayGetTypeID() -> CFTypeID {
    VTF_TYPE_ARRAY
}

#[no_mangle]
pub unsafe extern "C" fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex {
    if the_array.is_null() || vtf_get_type_id(the_array) != VTF_TYPE_ARRAY {
        return 0;
    }
    (*(the_array as *const vtf_cf_array)).values.len() as CFIndex
}

#[no_mangle]
pub unsafe extern "C" fn CFArrayGetValueAtIndex(
    the_array: CFArrayRef,
    index: CFIndex,
) -> *const c_void {
    if the_array.is_null() || vtf_get_type_id(the_array) != VTF_TYPE_ARRAY || index < 0 {
        return ptr::null();
    }
    let array = &*(the_array as *const vtf_cf_array);
    array
        .values
        .get(index as usize)
        .copied()
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub unsafe extern "C" fn CFDictionaryCreate(
    _allocator: CFAllocatorRef,
    keys: *const *const c_void,
    values: *const *const c_void,
    num_values: CFIndex,
    _key_callbacks: *const CFDictionaryKeyCallBacks,
    _value_callbacks: *const CFDictionaryValueCallBacks,
) -> CFDictionaryRef {
    if num_values < 0 {
        return ptr::null();
    }
    let mut retained_keys = Vec::with_capacity(num_values as usize);
    let mut retained_values = Vec::with_capacity(num_values as usize);
    for index in 0..(num_values as usize) {
        let key = if keys.is_null() {
            ptr::null()
        } else {
            *keys.add(index)
        };
        let value = if values.is_null() {
            ptr::null()
        } else {
            *values.add(index)
        };
        retained_keys.push(crate::runtime::CFRetain(key));
        retained_values.push(crate::runtime::CFRetain(value));
    }
    Box::into_raw(Box::new(vtf_cf_dictionary {
        base: vtf_cf_object::init(VTF_TYPE_DICTIONARY, Some(vtf_finalize_dictionary)),
        keys: retained_keys,
        values: retained_values,
    })) as CFDictionaryRef
}

#[no_mangle]
pub unsafe extern "C" fn CFDictionaryCreateMutable(
    _allocator: CFAllocatorRef,
    capacity: CFIndex,
    _key_callbacks: *const CFDictionaryKeyCallBacks,
    _value_callbacks: *const CFDictionaryValueCallBacks,
) -> CFMutableDictionaryRef {
    let cap = capacity.max(0) as usize;
    Box::into_raw(Box::new(vtf_cf_dictionary {
        base: vtf_cf_object::init(VTF_TYPE_DICTIONARY, Some(vtf_finalize_dictionary)),
        keys: Vec::with_capacity(cap),
        values: Vec::with_capacity(cap),
    })) as CFMutableDictionaryRef
}

#[no_mangle]
pub unsafe extern "C" fn CFDictionarySetValue(
    dictionary: CFMutableDictionaryRef,
    key: *const c_void,
    value: *const c_void,
) {
    if dictionary.is_null() || vtf_get_type_id(dictionary) != VTF_TYPE_DICTIONARY {
        return;
    }
    let dictionary = dictionary as *mut vtf_cf_dictionary;
    if let Some(index) = vtf_dictionary_find_index(dictionary, key) {
        crate::runtime::CFRelease((&(*dictionary).values)[index]);
        (&mut (*dictionary).values)[index] = crate::runtime::CFRetain(value);
        return;
    }
    (*dictionary).keys.push(crate::runtime::CFRetain(key));
    (*dictionary).values.push(crate::runtime::CFRetain(value));
}

#[no_mangle]
pub unsafe extern "C" fn CFDictionaryGetValue(
    dictionary: CFDictionaryRef,
    key: *const c_void,
) -> *const c_void {
    if dictionary.is_null() || vtf_get_type_id(dictionary) != VTF_TYPE_DICTIONARY {
        return ptr::null();
    }
    let dictionary = dictionary as *const vtf_cf_dictionary;
    vtf_dictionary_find_index(dictionary, key)
        .map(|index| (&(*dictionary).values)[index])
        .unwrap_or(ptr::null())
}

#[no_mangle]
pub unsafe extern "C" fn CFDictionaryGetValueIfPresent(
    dictionary: CFDictionaryRef,
    key: *const c_void,
    value: *mut *const c_void,
) -> Boolean {
    let found = CFDictionaryGetValue(dictionary, key);
    if found.is_null() {
        return 0;
    }
    if !value.is_null() {
        *value = found;
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn CFDictionaryContainsKey(
    dictionary: CFDictionaryRef,
    key: *const c_void,
) -> Boolean {
    if dictionary.is_null() || vtf_get_type_id(dictionary) != VTF_TYPE_DICTIONARY {
        return 0;
    }
    let dictionary = dictionary as *const vtf_cf_dictionary;
    if vtf_dictionary_find_index(dictionary, key).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn CFDictionaryGetTypeID() -> CFTypeID {
    VTF_TYPE_DICTIONARY
}

#[no_mangle]
pub unsafe extern "C" fn CFDictionaryCreateCopy(
    _allocator: CFAllocatorRef,
    dictionary: CFDictionaryRef,
) -> CFDictionaryRef {
    if dictionary.is_null() || vtf_get_type_id(dictionary) != VTF_TYPE_DICTIONARY {
        return ptr::null();
    }
    let dictionary = &*(dictionary as *const vtf_cf_dictionary);
    let mut copied_keys = Vec::with_capacity(dictionary.keys.len());
    let mut copied_values = Vec::with_capacity(dictionary.values.len());
    for key in &dictionary.keys {
        copied_keys.push(crate::runtime::CFRetain(*key));
    }
    for value in &dictionary.values {
        copied_values.push(crate::runtime::CFRetain(*value));
    }
    Box::into_raw(Box::new(vtf_cf_dictionary {
        base: vtf_cf_object::init(VTF_TYPE_DICTIONARY, Some(vtf_finalize_dictionary)),
        keys: copied_keys,
        values: copied_values,
    })) as CFDictionaryRef
}

#[no_mangle]
pub unsafe extern "C" fn CFBooleanGetValue(boolean_value: CFBooleanRef) -> Boolean {
    if boolean_value.is_null() || vtf_get_type_id(boolean_value) != VTF_TYPE_BOOLEAN {
        return 0;
    }
    if (*(boolean_value as *const vtf_cf_boolean)).value {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// Build a NUL-terminated C string we can pass to
    /// `VTF_CFStringCreateStatic` and keep alive for the test's lifetime.
    fn cstr(value: &'static str) -> CString {
        CString::new(value).unwrap()
    }

    #[test]
    fn cfstring_get_cstring_roundtrip() {
        unsafe {
            let owned = cstr("AVVideoCodecKey");
            let cf = VTF_CFStringCreateStatic(owned.as_ptr());
            assert!(!cf.is_null());
            assert_eq!(CFStringGetLength(cf) as usize, "AVVideoCodecKey".len());

            let mut buf = [0u8; 32];
            let ok = CFStringGetCString(
                cf,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as CFIndex,
                kCFStringEncodingUTF8,
            );
            assert_eq!(ok, 1);
            let written = CStr::from_ptr(buf.as_ptr() as *const c_char)
                .to_str()
                .unwrap();
            assert_eq!(written, "AVVideoCodecKey");

            crate::runtime::CFRelease(cf);
        }
    }

    #[test]
    fn cfstring_get_cstring_rejects_short_buffer() {
        unsafe {
            let owned = cstr("hello");
            let cf = VTF_CFStringCreateStatic(owned.as_ptr());
            // Buffer needs len("hello") + 1 = 6 bytes; pass 4.
            let mut buf = [0u8; 4];
            let ok = CFStringGetCString(
                cf,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as CFIndex,
                kCFStringEncodingUTF8,
            );
            assert_eq!(ok, 0, "short buffer must return false");
            crate::runtime::CFRelease(cf);
        }
    }

    #[test]
    fn cfnumber_int_roundtrip() {
        unsafe {
            let value: i32 = 42;
            let num =
                CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT32_TYPE, &value as *const _ as _);
            assert!(!num.is_null());
            assert_eq!(CFNumberGetType(num), K_CF_NUMBER_SINT32_TYPE);
            assert_eq!(CFGetTypeID(num), CFNumberGetTypeID());
            let mut out: i32 = 0;
            let ok = CFNumberGetValue(num, K_CF_NUMBER_SINT32_TYPE, &mut out as *mut _ as _);
            assert_eq!(ok, 1);
            assert_eq!(out, 42);
            crate::runtime::CFRelease(num);
        }
    }

    #[test]
    fn cfnumber_int64_roundtrip() {
        unsafe {
            let value: i64 = 0x0123_4567_89AB_CDEF;
            let num =
                CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT64_TYPE, &value as *const _ as _);
            let mut out: i64 = 0;
            let ok = CFNumberGetValue(num, K_CF_NUMBER_SINT64_TYPE, &mut out as *mut _ as _);
            assert_eq!(ok, 1);
            assert_eq!(out, value);
            crate::runtime::CFRelease(num);
        }
    }

    #[test]
    fn cfnumber_double_roundtrip() {
        unsafe {
            let value: f64 = 30.0;
            let num = CFNumberCreate(ptr::null(), K_CF_NUMBER_DOUBLE_TYPE, &value as *const _ as _);
            let mut out: f64 = 0.0;
            let ok = CFNumberGetValue(num, K_CF_NUMBER_DOUBLE_TYPE, &mut out as *mut _ as _);
            assert_eq!(ok, 1);
            assert_eq!(out, 30.0);
            crate::runtime::CFRelease(num);
        }
    }

    #[test]
    fn cfnumber_rejects_unknown_type() {
        unsafe {
            let value: i32 = 1;
            let num = CFNumberCreate(ptr::null(), 9999, &value as *const _ as _);
            assert!(num.is_null());
        }
    }

    #[test]
    fn cfdictionary_set_get_roundtrip() {
        unsafe {
            let dict = CFDictionaryCreateMutable(ptr::null(), 0, ptr::null(), ptr::null());
            assert!(!dict.is_null());

            let key_owned = cstr("ProfileLevel");
            let key = VTF_CFStringCreateStatic(key_owned.as_ptr());
            let value: i32 = 256;
            let val = CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT32_TYPE, &value as *const _ as _);

            CFDictionarySetValue(dict, key, val);

            let got = CFDictionaryGetValue(dict, key);
            assert!(!got.is_null(), "key just inserted must be found");
            let mut out: i32 = 0;
            CFNumberGetValue(got, K_CF_NUMBER_SINT32_TYPE, &mut out as *mut _ as _);
            assert_eq!(out, 256);

            crate::runtime::CFRelease(key);
            crate::runtime::CFRelease(val);
            crate::runtime::CFRelease(dict);
        }
    }

    #[test]
    fn cfdictionary_set_value_replaces_existing() {
        unsafe {
            let dict = CFDictionaryCreateMutable(ptr::null(), 0, ptr::null(), ptr::null());
            let key_owned = cstr("k");
            let key = VTF_CFStringCreateStatic(key_owned.as_ptr());

            let v1: i32 = 1;
            let val1 = CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT32_TYPE, &v1 as *const _ as _);
            CFDictionarySetValue(dict, key, val1);

            let v2: i32 = 2;
            let val2 = CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT32_TYPE, &v2 as *const _ as _);
            CFDictionarySetValue(dict, key, val2);

            let got = CFDictionaryGetValue(dict, key);
            let mut out: i32 = 0;
            CFNumberGetValue(got, K_CF_NUMBER_SINT32_TYPE, &mut out as *mut _ as _);
            assert_eq!(out, 2, "second set must replace the first");

            crate::runtime::CFRelease(val1);
            crate::runtime::CFRelease(val2);
            crate::runtime::CFRelease(key);
            crate::runtime::CFRelease(dict);
        }
    }

    #[test]
    fn cfdictionary_contains_key() {
        unsafe {
            let dict = CFDictionaryCreateMutable(ptr::null(), 0, ptr::null(), ptr::null());
            let here_owned = cstr("here");
            let missing_owned = cstr("missing");
            let key = VTF_CFStringCreateStatic(here_owned.as_ptr());
            let missing = VTF_CFStringCreateStatic(missing_owned.as_ptr());
            let v: i32 = 7;
            let val = CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT32_TYPE, &v as *const _ as _);
            CFDictionarySetValue(dict, key, val);

            assert_eq!(CFDictionaryContainsKey(dict, key), 1);
            assert_eq!(CFDictionaryContainsKey(dict, missing), 0);

            crate::runtime::CFRelease(key);
            crate::runtime::CFRelease(missing);
            crate::runtime::CFRelease(val);
            crate::runtime::CFRelease(dict);
        }
    }

    #[test]
    fn cfdictionary_get_value_if_present() {
        unsafe {
            let dict = CFDictionaryCreateMutable(ptr::null(), 0, ptr::null(), ptr::null());
            let key_owned = cstr("present");
            let missing_owned = cstr("absent");
            let key = VTF_CFStringCreateStatic(key_owned.as_ptr());
            let missing = VTF_CFStringCreateStatic(missing_owned.as_ptr());
            let v: i32 = 99;
            let val = CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT32_TYPE, &v as *const _ as _);
            CFDictionarySetValue(dict, key, val);

            let mut out: *const c_void = ptr::null();
            assert_eq!(CFDictionaryGetValueIfPresent(dict, key, &mut out), 1);
            assert!(!out.is_null());
            assert_eq!(out, val);

            // Missing key — out parameter is left untouched on failure.
            let mut sentinel: *const c_void = 0xdead_beef as *const c_void;
            assert_eq!(
                CFDictionaryGetValueIfPresent(dict, missing, &mut sentinel),
                0
            );
            assert_eq!(
                sentinel, 0xdead_beef as *const c_void,
                "miss must not overwrite the out parameter"
            );

            crate::runtime::CFRelease(key);
            crate::runtime::CFRelease(missing);
            crate::runtime::CFRelease(val);
            crate::runtime::CFRelease(dict);
        }
    }

    #[test]
    fn cfdictionary_create_copy_preserves_entries() {
        unsafe {
            let src = CFDictionaryCreateMutable(ptr::null(), 0, ptr::null(), ptr::null());
            let k_owned = cstr("k");
            let key = VTF_CFStringCreateStatic(k_owned.as_ptr());
            let v: i32 = 42;
            let val = CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT32_TYPE, &v as *const _ as _);
            CFDictionarySetValue(src, key, val);

            let copy = CFDictionaryCreateCopy(ptr::null(), src);
            assert!(!copy.is_null());
            assert_ne!(copy, src, "copy must be a distinct allocation");

            let from_copy = CFDictionaryGetValue(copy, key);
            assert_eq!(from_copy, val, "copy must surface the same value pointer");

            crate::runtime::CFRelease(src);
            crate::runtime::CFRelease(copy);
            crate::runtime::CFRelease(key);
            crate::runtime::CFRelease(val);
        }
    }

    #[test]
    fn cfdictionary_create_with_arrays() {
        unsafe {
            let k_owned = cstr("Codec");
            let key = VTF_CFStringCreateStatic(k_owned.as_ptr());
            let v: i32 = 1;
            let val = CFNumberCreate(ptr::null(), K_CF_NUMBER_SINT32_TYPE, &v as *const _ as _);

            let keys = [key];
            let values = [val];
            let dict = CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr() as *const *const c_void,
                values.as_ptr() as *const *const c_void,
                1,
                ptr::null(),
                ptr::null(),
            );
            assert!(!dict.is_null());
            assert_eq!(CFDictionaryGetValue(dict, key), val);

            crate::runtime::CFRelease(dict);
            crate::runtime::CFRelease(key);
            crate::runtime::CFRelease(val);
        }
    }

    #[test]
    fn cfboolean_globals_return_expected_values() {
        unsafe {
            assert_eq!(CFBooleanGetValue(kCFBooleanTrue.0), 1);
            assert_eq!(CFBooleanGetValue(kCFBooleanFalse.0), 0);
        }
    }
}
