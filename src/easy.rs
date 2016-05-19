//! Bindings to the "easy" libcurl API.
//!
//! This module contains some simple types like `Easy` and `List` which are just
//! wrappers around the corresponding libcurl types. There's also a few enums
//! scattered about for various options here and there.
//!
//! Most simple usage of libcurl will likely use the `Easy` structure here, and
//! you can find more docs about its usage on that struct.

use std::cell::Cell;
use std::ffi::{CString, CStr};
use std::io::SeekFrom;
use std::marker;
use std::path::Path;
use std::slice;
use std::str;
use std::time::Duration;

use curl_sys;
use libc::{self, c_long, c_int, c_char, c_void, size_t, c_double};

use Error;
use panic;

// TODO: checked casts everywhere

/// Raw bindings to a libcurl "easy session".
///
/// This type corresponds to the `CURL` type in libcurl, and is probably what
/// you want for just sending off a simple HTTP request and fetching a response.
/// Each easy handle can be thought of as a large builder before calling the
/// final `perform` function.
///
/// There are many many configuration options for each `Easy` handle, and they
/// should all have their own documentation indicating what it affects and how
/// it interacts with other options. Some implementations of libcurl can use
/// this handle to interact with many different protocols, although by default
/// this crate only guarantees the HTTP/HTTPS protocols working.
///
/// This struct's associated lifetime indicates the lifetime of various pieces
/// of borrowed data that can be configured. Examples of this are callbacks,
/// POST data, and HTTP headers. The associated lifetime can be "reset" through
/// the `reset_lifetime` function to allow changing the scope of parameters on a
/// handle.
///
/// Note that almost all methods on this structure which configure various
/// properties return a `Result`. This is largely used to detect whether the
/// underlying implementation of libcurl actually implements the option being
/// requested. If you're linked to a version of libcurl which doesn't support
/// the option, then an error will be returned. Some options also perform some
/// validation when they're set, and the error is returned through this vector.
///
/// ## Examples
///
/// Creating a handle which can be used later
///
/// ```
/// use curl::easy::Easy;
///
/// let handle = Easy::new();
/// ```
///
/// Send an HTTP request, writing the response to stdout.
///
/// ```
/// use curl::easy::Easy;
///
/// let mut handle = Easy::new();
/// handle.url("https://www.rust-lang.org/").unwrap();
/// handle.perform().unwrap();
/// ```
///
/// Collect all output of an HTTP request to a vector.
///
/// ```
/// use curl::easy::Easy;
///
/// let mut data = Vec::new();
/// {
///     let mut callback = |d: &[u8]| {
///         data.extend_from_slice(d);
///         d.len()
///     };
///     let mut handle = Easy::new();
///     handle.url("https://www.rust-lang.org/").unwrap();
///     handle.write_function(&mut callback).unwrap();
///     handle.perform().unwrap();
/// }
/// println!("{:?}", data);
/// ```
///
/// More examples of various properties of an HTTP request can be found on the
/// specific methods as well.
pub struct Easy<'a> {
    handle: *mut curl_sys::CURL,
    _marker: marker::PhantomData<Cell<&'a i32>>,
    data: Box<EasyData<'a>>,
}

struct EasyData<'a> {
    write: Box<FnMut(&[u8]) -> usize + Send + 'a>,
    read: Box<FnMut(&mut [u8]) -> usize + Send + 'a>,
    seek: Box<FnMut(SeekFrom) -> SeekResult + Send + 'a>,
    debug: Box<FnMut(InfoType, &[u8]) + Send + 'a>,
    header: Box<FnMut(&[u8]) -> bool + Send + 'a>,
    progress: Box<FnMut(f64, f64, f64, f64) -> bool + Send + 'a>,
    header_list: List,
}

// libcurl guarantees that a CURL handle is fine to be transferred so long as
// it's not used concurrently, and we do that correctly ourselves.
unsafe impl<'a> Send for Easy<'a> {}

// This type is `Sync` as it adheres to the proper `self` conventions, which in
// this case for libcurl basically means that everything takes `&mut self`.
unsafe impl<'a> Sync for Easy<'a> {}

/// Possible proxy types that libcurl currently understands.
#[allow(missing_docs)]
pub enum ProxyType {
    Http = curl_sys::CURLPROXY_HTTP as isize,
    Http1 = curl_sys::CURLPROXY_HTTP_1_0 as isize,
    Socks4 = curl_sys::CURLPROXY_SOCKS4 as isize,
    Socks5 = curl_sys::CURLPROXY_SOCKS5 as isize,
    Socks4a = curl_sys::CURLPROXY_SOCKS4A as isize,
    Socks5Hostname = curl_sys::CURLPROXY_SOCKS5_HOSTNAME as isize,

    /// Hidden variant to indicate that this enum should not be matched on, it
    /// may grow over time.
    #[doc(hidden)]
    __Nonexhaustive,
}

/// Possible conditions for the `time_condition` method.
#[allow(missing_docs)]
pub enum TimeCondition {
    None = curl_sys::CURL_TIMECOND_NONE as isize,
    IfModifiedSince = curl_sys::CURL_TIMECOND_IFMODSINCE as isize,
    IfUnmodifiedSince = curl_sys::CURL_TIMECOND_IFUNMODSINCE as isize,
    LastModified = curl_sys::CURL_TIMECOND_LASTMOD as isize,

    /// Hidden variant to indicate that this enum should not be matched on, it
    /// may grow over time.
    #[doc(hidden)]
    __Nonexhaustive,
}

/// Possible values to pass to the `ip_resolve` method.
#[allow(missing_docs)]
pub enum IpResolve {
    V4 = curl_sys::CURL_IPRESOLVE_V4 as isize,
    V6 = curl_sys::CURL_IPRESOLVE_V6 as isize,
    Any = curl_sys::CURL_IPRESOLVE_WHATEVER as isize,

    /// Hidden variant to indicate that this enum should not be matched on, it
    /// may grow over time.
    #[doc(hidden)]
    __Nonexhaustive = 500,
}

/// Possible values to pass to the `ip_resolve` method.
#[allow(missing_docs)]
pub enum SslVersion {
    Default = curl_sys::CURL_SSLVERSION_DEFAULT as isize,
    Tlsv1 = curl_sys::CURL_SSLVERSION_TLSv1 as isize,
    Sslv2 = curl_sys::CURL_SSLVERSION_SSLv2 as isize,
    Sslv3 = curl_sys::CURL_SSLVERSION_SSLv3 as isize,
    // Tlsv10 = curl_sys::CURL_SSLVERSION_TLSv1_0 as isize,
    // Tlsv11 = curl_sys::CURL_SSLVERSION_TLSv1_1 as isize,
    // Tlsv12 = curl_sys::CURL_SSLVERSION_TLSv1_2 as isize,

    /// Hidden variant to indicate that this enum should not be matched on, it
    /// may grow over time.
    #[doc(hidden)]
    __Nonexhaustive = 500,
}

/// Possible return values from the `seek_function` callback.
pub enum SeekResult {
    /// Indicates that the seek operation was a success
    Ok = curl_sys::CURL_SEEKFUNC_OK as isize,

    /// Indicates that the seek operation failed, and the entire request should
    /// fail as a result.
    Fail = curl_sys::CURL_SEEKFUNC_FAIL as isize,

    /// Indicates that although the seek failed libcurl should attempt to keep
    /// working if possible (for example "seek" through reading).
    CantSeek = curl_sys::CURL_SEEKFUNC_CANTSEEK as isize,

    /// Hidden variant to indicate that this enum should not be matched on, it
    /// may grow over time.
    #[doc(hidden)]
    __Nonexhaustive = 500,
}

/// Possible data chunks that can be witnessed as part of the `debug_function`
/// callback.
pub enum InfoType {
    /// The data is informational text.
    Text,

    /// The data is header (or header-like) data received from the peer.
    HeaderIn,

    /// The data is header (or header-like) data sent to the peer.
    HeaderOut,

    /// The data is protocol data received from the peer.
    DataIn,

    /// The data is protocol data sent to the peer.
    DataOut,

    /// The data is SSL/TLS (binary) data received from the peer.
    SslDataIn,

    /// The data is SSL/TLS (binary) data sent to the peer.
    SslDataOut,

    /// Hidden variant to indicate that this enum should not be matched on, it
    /// may grow over time.
    #[doc(hidden)]
    __Nonexhaustive,
}

/// A linked list of a strings
pub struct List {
    raw: *mut curl_sys::curl_slist,
}

/// An iterator over `List`
pub struct Iter<'a> {
    _me: &'a List,
    cur: *mut curl_sys::curl_slist,
}

unsafe impl Send for List {}

fn cvt(r: curl_sys::CURLcode) -> Result<(), Error> {
    if r == curl_sys::CURLE_OK {
        Ok(())
    } else {
        Err(Error::new(r))
    }
}

impl<'a> Easy<'a> {
    /// Creates a new "easy" handle which is the core of almost all operations
    /// in libcurl.
    ///
    /// To use a handle, applications typically configure a number of options
    /// followed by a call to `perform`. Options are preserved across calls to
    /// `perform` and need to be reset manually (or via the `reset` method) if
    /// this is not desired.
    pub fn new() -> Easy<'a> {
        ::init();
        unsafe {
            let handle = curl_sys::curl_easy_init();
            assert!(!handle.is_null());
            let mut ret = Easy {
                handle: handle,
                data: blank_data(),
                _marker: marker::PhantomData,
            };
            default_configure(&mut ret);
            return ret
        }
    }

    // =========================================================================
    // Behavior options

    /// Configures this handle to have verbose output to help debug protocol
    /// information.
    ///
    /// By default output goes to stderr, but the `stderr` function on this type
    /// can configure that. You can also use the `debug_function` method to get
    /// all protocol data sent and received.
    ///
    /// By default, this option is `false`.
    pub fn verbose(&mut self, verbose: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_VERBOSE, verbose as c_long)
    }

    /// Indicates whether header information is streamed to the output body of
    /// this request.
    ///
    /// This option is only relevant for protocols which have header metadata
    /// (like http or ftp). It's not generally possible to extract headers
    /// from the body if using this method, that use case should be intended for
    /// the `header_function` method.
    ///
    /// To set HTTP headers, use the `http_header` method.
    ///
    /// By default, this option is `false` and corresponds to
    /// `CURLOPT_HEADER`.
    pub fn show_header(&mut self, show: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_HEADER, show as c_long)
    }

    /// Indicates whether a progress meter will be shown for requests done with
    /// this handle.
    ///
    /// This will also prevent the `progress_function` from being called.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_NOPROGRESS`.
    pub fn progress(&mut self, progress: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_NOPROGRESS,
                         (!progress) as c_long)
    }

    /// Inform libcurl whether or not it should install signal handlers or
    /// attempt to use signals to perform library functions.
    ///
    /// If this option is disabled then timeouts during name resolution will not
    /// work unless libcurl is built against c-ares. Note that enabling this
    /// option, however, may not cause libcurl to work with multiple threads.
    ///
    /// By default this option is `false` and corresponds to `CURLOPT_NOSIGNAL`.
    /// Note that this default is **different than libcurl** as it is intended
    /// that this library is threadsafe by default. See the [libcurl docs] for
    /// some more information.
    ///
    /// [libcurl docs]: https://curl.haxx.se/libcurl/c/threadsafe.html
    pub fn signal(&mut self, signal: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_NOSIGNAL,
                         (!signal) as c_long)
    }

    /// Indicates whether multiple files will be transferred based on the file
    /// name pattern.
    ///
    /// The last part of a filename uses fnmatch-like pattern matching.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_WILDCARDMATCH`.
    pub fn wildcard_match(&mut self, m: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_WILDCARDMATCH, m as c_long)
    }

    // =========================================================================
    // Callback options

    /// Set callback for writing received data.
    ///
    /// This callback function gets called by libcurl as soon as there is data
    /// received that needs to be saved.
    ///
    /// The callback function will be passed as much data as possible in all
    /// invokes, but you must not make any assumptions. It may be one byte, it
    /// may be thousands. If `show_header` is enabled, which makes header data
    /// get passed to the write callback, you can get up to
    /// `CURL_MAX_HTTP_HEADER` bytes of header data passed into it.  This
    /// usually means 100K.
    ///
    /// This function may be called with zero bytes data if the transferred file
    /// is empty.
    ///
    /// The callback should return the number of bytes actually taken care of.
    /// If that amount differs from the amount passed to your callback function,
    /// it'll signal an error condition to the library. This will cause the
    /// transfer to get aborted and the libcurl function used will return
    /// an error with `is_write_error`.
    ///
    /// If your callback function returns `CURL_WRITEFUNC_PAUSE` it will cause
    /// this transfer to become paused. See `pause` for further details.
    ///
    /// By default data is written to stdout, and this corresponds to the
    /// `CURLOPT_WRITEFUNCTION` and `CURLOPT_WRITEDATA` options.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::*;
    /// use curl::easy::Easy;
    ///
    /// let mut handle = Easy::new();
    /// handle.url("https://www.rust-lang.org/").unwrap();
    /// handle.write_function(|data| {
    ///     stdout().write(data).unwrap_or(0)
    /// }).unwrap();
    /// handle.perform().unwrap();
    /// ```
    pub fn write_function<F>(&mut self, f: F) -> Result<(), Error>
        where F: FnMut(&[u8]) -> usize + Send + 'a
    {
        self._write_function(Box::new(f))
    }

    fn _write_function(&mut self, f: Box<FnMut(&[u8]) -> usize + Send + 'a>)
                       -> Result<(), Error> {
        self.data.write = f;
        try!(self.setopt_ptr(curl_sys::CURLOPT_WRITEFUNCTION,
                             cb as usize as *const c_char));
        let ptr = &*self.data as *const _;
        try!(self.setopt_ptr(curl_sys::CURLOPT_WRITEDATA,
                             ptr as *const c_char));
        return Ok(());

        unsafe extern fn cb(ptr: *mut c_char,
                            size: size_t,
                            nmemb: size_t,
                            data: *mut c_void) -> size_t {
            let input = slice::from_raw_parts(ptr as *const u8, size * nmemb);
            panic::catch(|| {
                ((*(data as *mut EasyData)).write)(input)
            }).unwrap_or(!0)
        }
    }

    /// Read callback for data uploads.
    ///
    /// This callback function gets called by libcurl as soon as it needs to
    /// read data in order to send it to the peer - like if you ask it to upload
    /// or post data to the server.
    ///
    /// Your function must then return the actual number of bytes that it stored
    /// in that memory area. Returning 0 will signal end-of-file to the library
    /// and cause it to stop the current transfer.
    ///
    /// If you stop the current transfer by returning 0 "pre-maturely" (i.e
    /// before the server expected it, like when you've said you will upload N
    /// bytes and you upload less than N bytes), you may experience that the
    /// server "hangs" waiting for the rest of the data that won't come.
    ///
    /// The read callback may return `CURL_READFUNC_ABORT` to stop the current
    /// operation immediately, resulting in a `is_aborted_by_callback` error
    /// code from the transfer.
    ///
    /// The callback can return `CURL_READFUNC_PAUSE` to cause reading from this
    /// connection to pause. See `pause` for further details.
    ///
    /// By default data is read from stdin, and this corresponds to the
    /// `CURLOPT_READFUNCTION` and `CURLOPT_READDATA` options.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::*;
    /// use curl::easy::Easy;
    ///
    /// let mut data_to_upload = &b"foobar"[..];
    /// let mut handle = Easy::new();
    /// handle.url("https://example.com/login").unwrap();
    /// handle.read_function(|into| {
    ///     data_to_upload.read(into).unwrap_or(0)
    /// }).unwrap();
    /// handle.post(true).unwrap();
    /// handle.perform().unwrap();
    /// ```
    pub fn read_function<F>(&mut self, f: F) -> Result<(), Error>
        where F: FnMut(&mut [u8]) -> usize + Send + 'a
    {
        self._read_function(Box::new(f))
    }

    fn _read_function(&mut self, f: Box<FnMut(&mut [u8]) -> usize + Send + 'a>)
                      -> Result<(), Error> {
        self.data.read = f;
        try!(self.setopt_ptr(curl_sys::CURLOPT_READFUNCTION,
                             cb as usize as *const c_char));
        let ptr = &*self.data as *const _;
        try!(self.setopt_ptr(curl_sys::CURLOPT_READDATA,
                             ptr as *const c_char));
        return Ok(());

        unsafe extern fn cb(ptr: *mut c_char,
                            size: size_t,
                            nmemb: size_t,
                            data: *mut c_void) -> size_t {
            let input = slice::from_raw_parts_mut(ptr as *mut u8, size * nmemb);
            panic::catch(|| {
                ((*(data as *mut EasyData)).read)(input)
            }).unwrap_or(!0)
        }
    }

    /// User callback for seeking in input stream.
    ///
    /// This function gets called by libcurl to seek to a certain position in
    /// the input stream and can be used to fast forward a file in a resumed
    /// upload (instead of reading all uploaded bytes with the normal read
    /// function/callback). It is also called to rewind a stream when data has
    /// already been sent to the server and needs to be sent again. This may
    /// happen when doing a HTTP PUT or POST with a multi-pass authentication
    /// method, or when an existing HTTP connection is reused too late and the
    /// server closes the connection.
    ///
    /// The callback function must return `SeekResult::Ok` on success,
    /// `SeekResult::Fail` to cause the upload operation to fail or
    /// `SeekResult::CantSeek` to indicate that while the seek failed, libcurl
    /// is free to work around the problem if possible. The latter can sometimes
    /// be done by instead reading from the input or similar.
    ///
    /// By default data this option is not set, and this corresponds to the
    /// `CURLOPT_SEEKFUNCTION` and `CURLOPT_SEEKDATA` options.
    pub fn seek_function<F>(&mut self, f: F) -> Result<(), Error>
        where F: FnMut(SeekFrom) -> SeekResult + Send + 'a
    {
        self._seek_function(Box::new(f))
    }

    fn _seek_function(&mut self, f: Box<FnMut(SeekFrom) -> SeekResult + Send + 'a>)
                      -> Result<(), Error> {
        self.data.seek = f;
        try!(self.setopt_ptr(curl_sys::CURLOPT_SEEKFUNCTION,
                             cb as usize as *const c_char));
        let ptr = &*self.data as *const _;
        try!(self.setopt_ptr(curl_sys::CURLOPT_SEEKDATA,
                             ptr as *const c_char));
        return Ok(());

        unsafe extern fn cb(data: *mut c_void,
                            offset: curl_sys::curl_off_t,
                            origin: c_int) -> c_int {
            panic::catch(|| {
                let from = if origin == libc::SEEK_SET {
                    SeekFrom::Start(offset as u64)
                } else {
                    panic!("unknown origin from libcurl: {}", origin);
                };
                ((*(data as *mut EasyData)).seek)(from) as c_int
            }).unwrap_or(!0)
        }
    }

    /// Callback to progress meter function
    ///
    /// This function gets called by libcurl instead of its internal equivalent
    /// with a frequent interval. While data is being transferred it will be
    /// called very frequently, and during slow periods like when nothing is
    /// being transferred it can slow down to about one call per second.
    ///
    /// The callback gets told how much data libcurl will transfer and has
    /// transferred, in number of bytes. The first argument is the total number
    /// of bytes libcurl expects to download in this transfer. The second
    /// argument is the number of bytes downloaded so far. The third argument is
    /// the total number of bytes libcurl expects to upload in this transfer.
    /// The fourth argument is the number of bytes uploaded so far.
    ///
    /// Unknown/unused argument values passed to the callback will be set to
    /// zero (like if you only download data, the upload size will remain 0).
    /// Many times the callback will be called one or more times first, before
    /// it knows the data sizes so a program must be made to handle that.
    ///
    /// Returning `false` from this callback will cause libcurl to abort the
    /// transfer and return `is_aborted_by_callback`.
    ///
    /// If you transfer data with the multi interface, this function will not be
    /// called during periods of idleness unless you call the appropriate
    /// libcurl function that performs transfers.
    ///
    /// `noprogress` must be set to 0 to make this function actually get
    /// called.
    ///
    /// By default this function calls an internal method and corresponds to
    /// `CURLOPT_XFERINFOFUNCTION` and `CURLOPT_XFERINFODATA`.
    pub fn progress_function<F>(&mut self, f: F) -> Result<(), Error>
        where F: FnMut(f64, f64, f64, f64) -> bool + Send + 'a
    {
        self._progress_function(Box::new(f))
    }

    fn _progress_function(&mut self,
                          f: Box<FnMut(f64, f64, f64, f64) -> bool + Send + 'a>)
                          -> Result<(), Error> {
        self.data.progress = f;
        try!(self.setopt_ptr(curl_sys::CURLOPT_PROGRESSFUNCTION,
                             cb as usize as *const c_char));
        let ptr = &*self.data as *const _;
        try!(self.setopt_ptr(curl_sys::CURLOPT_PROGRESSDATA,
                             ptr as *const c_char));
        return Ok(());

        unsafe extern fn cb(data: *mut c_void,
                            dltotal: c_double,
                            dlnow: c_double,
                            ultotal: c_double,
                            ulnow: c_double) -> c_int {
            let keep_going = panic::catch(|| {
                let data = &mut *(data as *mut EasyData);
                (data.progress)(dltotal, dlnow, ultotal, ulnow)
            }).unwrap_or(false);
            if keep_going {
                0
            } else {
                1
            }
        }
    }

    /// Specify a debug callback
    ///
    /// `debug_function` replaces the standard debug function used when
    /// `verbose` is in effect. This callback receives debug information,
    /// as specified in the type argument.
    ///
    /// By default this option is not set and corresponds to the
    /// `CURLOPT_DEBUGFUNCTION` and `CURLOPT_DEBUGDATA` options.
    pub fn debug_function<F>(&mut self, f: F) -> Result<(), Error>
        where F: FnMut(InfoType, &[u8]) + Send + 'a
    {
        self._debug_function(Box::new(f))
    }

    fn _debug_function(&mut self, f: Box<FnMut(InfoType, &[u8]) + Send + 'a>)
                       -> Result<(), Error> {
        self.data.debug = f;
        try!(self.setopt_ptr(curl_sys::CURLOPT_DEBUGFUNCTION,
                             cb as usize as *const c_char));
        let ptr = &*self.data as *const _;
        try!(self.setopt_ptr(curl_sys::CURLOPT_DEBUGDATA,
                             ptr as *const c_char));
        return Ok(());

        // TODO: expose `handle`? is that safe?
        unsafe extern fn cb(_handle: *mut curl_sys::CURL,
                            kind: curl_sys::curl_infotype,
                            data: *mut c_char,
                            size: size_t,
                            userptr: *mut c_void) -> c_int {
            panic::catch(|| {
                let data = slice::from_raw_parts(data as *const u8, size);
                let kind = match kind {
                    curl_sys::CURLINFO_TEXT => InfoType::Text,
                    curl_sys::CURLINFO_HEADER_IN => InfoType::HeaderIn,
                    curl_sys::CURLINFO_HEADER_OUT => InfoType::HeaderOut,
                    curl_sys::CURLINFO_DATA_IN => InfoType::DataIn,
                    curl_sys::CURLINFO_DATA_OUT => InfoType::DataOut,
                    curl_sys::CURLINFO_SSL_DATA_IN => InfoType::SslDataIn,
                    curl_sys::CURLINFO_SSL_DATA_OUT => InfoType::SslDataOut,
                    _ => return,
                };
                ((*(userptr as *mut EasyData)).debug)(kind, data)
            });
            return 0
        }
    }

    /// Callback that receives header data
    ///
    /// This function gets called by libcurl as soon as it has received header
    /// data. The header callback will be called once for each header and only
    /// complete header lines are passed on to the callback. Parsing headers is
    /// very easy using this. If this callback returns `false` it'll signal an
    /// error to the library. This will cause the transfer to get aborted and
    /// the libcurl function in progress will return `is_write_error`.
    ///
    /// A complete HTTP header that is passed to this function can be up to
    /// CURL_MAX_HTTP_HEADER (100K) bytes.
    ///
    /// It's important to note that the callback will be invoked for the headers
    /// of all responses received after initiating a request and not just the
    /// final response. This includes all responses which occur during
    /// authentication negotiation. If you need to operate on only the headers
    /// from the final response, you will need to collect headers in the
    /// callback yourself and use HTTP status lines, for example, to delimit
    /// response boundaries.
    ///
    /// When a server sends a chunked encoded transfer, it may contain a
    /// trailer. That trailer is identical to a HTTP header and if such a
    /// trailer is received it is passed to the application using this callback
    /// as well. There are several ways to detect it being a trailer and not an
    /// ordinary header: 1) it comes after the response-body. 2) it comes after
    /// the final header line (CR LF) 3) a Trailer: header among the regular
    /// response-headers mention what header(s) to expect in the trailer.
    ///
    /// For non-HTTP protocols like FTP, POP3, IMAP and SMTP this function will
    /// get called with the server responses to the commands that libcurl sends.
    ///
    /// By default this option is not set and corresponds to the
    /// `CURLOPT_HEADERFUNCTION` and `CURLOPT_HEADERDATA` options.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::str;
    ///
    /// use curl::easy::Easy;
    ///
    /// let mut headers = Vec::new();
    /// {
    ///     let mut handle = Easy::new();
    ///     handle.url("https://www.rust-lang.org/").unwrap();
    ///     handle.header_function(|header| {
    ///         headers.push(str::from_utf8(header).unwrap().to_string());
    ///         true
    ///     }).unwrap();
    ///     handle.perform().unwrap();
    /// }
    ///
    /// println!("{:?}", headers);
    /// ```
    pub fn header_function<F>(&mut self, f: F) -> Result<(), Error>
        where F: FnMut(&[u8]) -> bool + Send + 'a
    {
        self._header_function(Box::new(f))
    }

    fn _header_function(&mut self, f: Box<FnMut(&[u8]) -> bool + Send + 'a>)
                        -> Result<(), Error> {
        self.data.header = f;
        try!(self.setopt_ptr(curl_sys::CURLOPT_HEADERFUNCTION,
                             cb as usize as *const c_char));
        let ptr = &*self.data as *const _;
        try!(self.setopt_ptr(curl_sys::CURLOPT_HEADERDATA,
                             ptr as *const c_char));
        return Ok(());

        unsafe extern fn cb(buffer: *mut c_char,
                            size: size_t,
                            nitems: size_t,
                            userptr: *mut c_void) -> size_t {
            let keep_going = panic::catch(|| {
                let data = slice::from_raw_parts(buffer as *const u8,
                                                 size * nitems);
                ((*(userptr as *mut EasyData)).header)(data)
            }).unwrap_or(false);
            if keep_going {
                size * nitems
            } else {
                !0
            }
        }
    }

    // =========================================================================
    // Error options

    // TODO: error buffer and stderr

    /// Indicates whether this library will fail on HTTP response codes >= 400.
    ///
    /// This method is not fail-safe especially when authentication is involved.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_FAILONERROR`.
    pub fn fail_on_error(&mut self, fail: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_FAILONERROR, fail as c_long)
    }

    // =========================================================================
    // Network options

    /// Provides the URL which this handle will work with.
    ///
    /// The string provided must be URL-encoded with the format:
    ///
    /// ```text
    /// scheme://host:port/path
    /// ```
    ///
    /// The syntax is not validated as part of this function and that is
    /// deferred until later.
    ///
    /// By default this option is not set and `perform` will not work until it
    /// is set. This option corresponds to `CURLOPT_URL`.
    pub fn url(&mut self, url: &str) -> Result<(), Error> {
        let url = try!(CString::new(url));
        self.setopt_str(curl_sys::CURLOPT_URL, &url)
    }

    /// Configures the port number to connect to, instead of the one specified
    /// in the URL or the default of the protocol.
    pub fn port(&mut self, port: u16) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_PORT, port as c_long)
    }

    // /// Indicates whether sequences of `/../` and `/./` will be squashed or not.
    // ///
    // /// By default this option is `false` and corresponds to
    // /// `CURLOPT_PATH_AS_IS`.
    // pub fn path_as_is(&mut self, as_is: bool) -> Result<(), Error> {
    // }

    /// Provide the URL of a proxy to use.
    ///
    /// By default this option is not set and corresponds to `CURLOPT_PROXY`.
    pub fn proxy(&mut self, url: &str) -> Result<(), Error> {
        let url = try!(CString::new(url));
        self.setopt_str(curl_sys::CURLOPT_PROXY, &url)
    }

    /// Provide port number the proxy is listening on.
    ///
    /// By default this option is not set (the default port for the proxy
    /// protocol is used) and corresponds to `CURLOPT_PROXYPORT`.
    pub fn proxy_port(&mut self, port: u16) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_PROXYPORT, port as c_long)
    }

    /// Indicates the type of proxy being used.
    ///
    /// By default this option is `ProxyType::Http` and corresponds to
    /// `CURLOPT_PROXYTYPE`.
    pub fn proxy_type(&mut self, kind: ProxyType) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_PROXYTYPE, kind as c_long)
    }

    /// Provide a list of hosts that should not be proxied to.
    ///
    /// This string is a comma-separated list of hosts which should not use the
    /// proxy specified for connections. A single `*` character is also accepted
    /// as a wildcard for all hosts.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_NOPROXY`.
    pub fn noproxy(&mut self, skip: &str) -> Result<(), Error> {
        let skip = try!(CString::new(skip));
        self.setopt_str(curl_sys::CURLOPT_PROXYTYPE, &skip)
    }

    /// Inform curl whether it should tunnel all operations through the proxy.
    ///
    /// This essentially means that a `CONNECT` is sent to the proxy for all
    /// outbound requests.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_HTTPPROXYTUNNEL`.
    pub fn http_proxy_tunnel(&mut self, tunnel: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_HTTPPROXYTUNNEL,
                         tunnel as c_long)
    }

    /// Tell curl which interface to bind to for an outgoing network interface.
    ///
    /// The interface name, IP address, or host name can be specified here.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_INTERFACE`.
    pub fn interface(&mut self, interface: &str) -> Result<(), Error> {
        let s = try!(CString::new(interface));
        self.setopt_str(curl_sys::CURLOPT_INTERFACE, &s)
    }

    /// Indicate which port should be bound to locally for this connection.
    ///
    /// By default this option is 0 (any port) and corresponds to
    /// `CURLOPT_LOCALPORT`.
    pub fn set_local_port(&mut self, port: u16) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_LOCALPORT, port as c_long)
    }

    /// Indicates the number of attempts libcurl will perform to find a working
    /// port number.
    ///
    /// By default this option is 1 and corresponds to
    /// `CURLOPT_LOCALPORTRANGE`.
    pub fn local_port_range(&mut self, range: u16) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_LOCALPORTRANGE,
                         range as c_long)
    }

    /// Sets the timeout of how long name resolves will be kept in memory.
    ///
    /// This is distinct from DNS TTL options and is entirely speculative.
    ///
    /// By default this option is 60s and corresponds to
    /// `CURLOPT_DNS_CACHE_TIMEOUT`.
    pub fn dns_cache_timeout(&mut self, dur: Duration) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_DNS_CACHE_TIMEOUT,
                         dur.as_secs() as c_long)
    }

    /// Specify the preferred receive buffer size, in bytes.
    ///
    /// This is treated as a request, not an order, and the main point of this
    /// is that the write callback may get called more often with smaller
    /// chunks.
    ///
    /// By default this option is the maximum write size and corresopnds to
    /// `CURLOPT_BUFFERSIZE`.
    pub fn buffer_size(&mut self, size: usize) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_BUFFERSIZE, size as c_long)
    }

    // /// Enable or disable TCP Fast Open
    // ///
    // /// By default this options defaults to `false` and corresponds to
    // /// `CURLOPT_TCP_FASTOPEN`
    // pub fn fast_open(&mut self, enable: bool) -> Result<(), Error> {
    // }

    /// Configures whether the TCP_NODELAY option is set, or Nagle's algorithm
    /// is disabled.
    ///
    /// The purpose of Nagle's algorithm is to minimize the number of small
    /// packet's on the network, and disabling this may be less efficient in
    /// some situations.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_TCP_NODELAY`.
    pub fn tcp_nodelay(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_TCP_NODELAY, enable as c_long)
    }

    // /// Configures whether TCP keepalive probes will be sent.
    // ///
    // /// The delay and frequency of these probes is controlled by `tcp_keepidle`
    // /// and `tcp_keepintvl`.
    // ///
    // /// By default this option is `false` and corresponds to
    // /// `CURLOPT_TCP_KEEPALIVE`.
    // pub fn tcp_keepalive(&mut self, enable: bool) -> Result<(), Error> {
    //     self.setopt_long(curl_sys::CURLOPT_TCP_KEEPALIVE, enable as c_long)
    // }

    // /// Configures the TCP keepalive idle time wait.
    // ///
    // /// This is the delay, after which the connection is idle, keepalive probes
    // /// will be sent. Not all operating systems support this.
    // ///
    // /// By default this corresponds to `CURLOPT_TCP_KEEPIDLE`.
    // pub fn tcp_keepidle(&mut self, amt: Duration) -> Result<(), Error> {
    //     self.setopt_long(curl_sys::CURLOPT_TCP_KEEPIDLE,
    //                      amt.as_secs() as c_long)
    // }
    //
    // /// Configures the delay between keepalive probes.
    // ///
    // /// By default this corresponds to `CURLOPT_TCP_KEEPINTVL`.
    // pub fn tcp_keepintvl(&mut self, amt: Duration) -> Result<(), Error> {
    //     self.setopt_long(curl_sys::CURLOPT_TCP_KEEPINTVL,
    //                      amt.as_secs() as c_long)
    // }

    /// Configures the scope for local IPv6 addresses.
    ///
    /// Sets the scope_id value to use when connecting to IPv6 or link-local
    /// addresses.
    ///
    /// By default this value is 0 and corresponds to `CURLOPT_ADDRESS_SCOPE`
    pub fn address_scope(&mut self, scope: u32) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_ADDRESS_SCOPE,
                         scope as c_long)
    }

    // =========================================================================
    // Names and passwords

    /// Configures the username to pass as authentication for this connection.
    ///
    /// By default this value is not set and corresponds to `CURLOPT_USERNAME`.
    pub fn username(&mut self, user: &str) -> Result<(), Error> {
        let user = try!(CString::new(user));
        self.setopt_str(curl_sys::CURLOPT_USERNAME, &user)
    }

    /// Configures the password to pass as authentication for this connection.
    ///
    /// By default this value is not set and corresponds to `CURLOPT_PASSWORD`.
    pub fn password(&mut self, pass: &str) -> Result<(), Error> {
        let pass = try!(CString::new(pass));
        self.setopt_str(curl_sys::CURLOPT_PASSWORD, &pass)
    }

    /// Configures the proxy username to pass as authentication for this
    /// connection.
    ///
    /// By default this value is not set and corresponds to
    /// `CURLOPT_PROXYUSERNAME`.
    pub fn proxy_username(&mut self, user: &str) -> Result<(), Error> {
        let user = try!(CString::new(user));
        self.setopt_str(curl_sys::CURLOPT_PROXYUSERNAME, &user)
    }

    /// Configures the proxy password to pass as authentication for this
    /// connection.
    ///
    /// By default this value is not set and corresponds to
    /// `CURLOPT_PROXYPASSWORD`.
    pub fn proxy_password(&mut self, pass: &str) -> Result<(), Error> {
        let pass = try!(CString::new(pass));
        self.setopt_str(curl_sys::CURLOPT_PROXYPASSWORD, &pass)
    }

    // =========================================================================
    // HTTP Options

    /// Indicates whether the referer header is automatically updated
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_AUTOREFERER`.
    pub fn autoreferer(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_AUTOREFERER, enable as c_long)
    }

    /// Enables automatic decompression of HTTP downloads.
    ///
    /// Sets the contents of the Accept-Encoding header sent in an HTTP request.
    /// This enables decoding of a response with Content-Encoding.
    ///
    /// Currently supported encoding are `identity`, `zlib`, and `gzip`. A
    /// zero-length string passed in will send all accepted encodings.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_ACCEPT_ENCODING`.
    pub fn accept_encoding(&mut self, encoding: &str) -> Result<(), Error> {
        let encoding = try!(CString::new(encoding));
        self.setopt_str(curl_sys::CURLOPT_ACCEPT_ENCODING, &encoding)
    }

    /// Request the HTTP Transfer Encoding.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_TRANSFER_ENCODING`.
    pub fn transfer_encoding(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_TRANSFER_ENCODING, enable as c_long)
    }

    /// Follow HTTP 3xx redirects.
    ///
    /// Indicates whether any `Location` headers in the response should get
    /// followed.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_FOLLOWLOCATION`.
    pub fn follow_location(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_FOLLOWLOCATION, enable as c_long)
    }

    /// Send credentials to hosts other than the first as well.
    ///
    /// Sends username/password credentials even when the host changes as part
    /// of a redirect.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_UNRESTRICTED_AUTH`.
    pub fn unrestricted_auth(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_UNRESTRICTED_AUTH, enable as c_long)
    }

    /// Set the maximum number of redirects allowed.
    ///
    /// A value of 0 will refuse any redirect.
    ///
    /// By default this option is `-1` (unlimited) and corresponds to
    /// `CURLOPT_MAXREDIRS`.
    pub fn max_redirections(&mut self, max: u32) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_MAXREDIRS, max as c_long)
    }

    // TODO: post_redirections

    /// Make an HTTP PUT request.
    ///
    /// By default this option is `false` and corresponds to `CURLOPT_PUT`.
    pub fn put(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_PUT, enable as c_long)
    }

    /// Make an HTTP POST request.
    ///
    /// This will also make the library use the
    /// `Content-Type: application/x-www-form-urlencoded` header.
    ///
    /// POST data can be specified through `post_fields` or by specifying a read
    /// function.
    ///
    /// By default this option is `false` and corresponds to `CURLOPT_POST`.
    pub fn post(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_POST, enable as c_long)
    }

    /// Configures the data that will be uploaded as part of a POST.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_POSTFIELDS`.
    pub fn post_fields(&mut self, data: &'a [u8]) -> Result<(), Error> {
        // Set the length before the pointer so libcurl knows how much to read
        try!(self.post_field_size(data.len() as u64));
        self.setopt_ptr(curl_sys::CURLOPT_POSTFIELDS,
                        data.as_ptr() as *const _)
    }

    /// Configures the data that will be uploaded as part of a POST.
    ///
    /// Note that the data is copied into this handle and if that's not desired
    /// then the read callbacks can be used instead.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_COPYPOSTFIELDS`.
    pub fn post_fields_copy(&mut self, data: &[u8]) -> Result<(), Error> {
        // Set the length before the pointer so libcurl knows how much to read
        try!(self.post_field_size(data.len() as u64));
        self.setopt_ptr(curl_sys::CURLOPT_COPYPOSTFIELDS,
                        data.as_ptr() as *const _)
    }

    /// Configures the size of data that's going to be uploaded as part of a
    /// POST operation.
    ///
    /// This is called automaticsally as part of `post_fields` and should only
    /// be called if data is being provided in a read callback (and even then
    /// it's optional).
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_POSTFIELDSIZE_LARGE`.
    pub fn post_field_size(&mut self, size: u64) -> Result<(), Error> {
        // Clear anything previous to ensure we don't read past a buffer
        try!(self.setopt_ptr(curl_sys::CURLOPT_POSTFIELDS, 0 as *const _));
        self.setopt_off_t(curl_sys::CURLOPT_POSTFIELDSIZE_LARGE,
                          size as curl_sys::curl_off_t)
    }

    // TODO: httppost (multipart post)

    /// Sets the HTTP referer header
    ///
    /// By default this option is not set and corresponds to `CURLOPT_REFERER`.
    pub fn referer(&mut self, referer: &str) -> Result<(), Error> {
        let referer = try!(CString::new(referer));
        self.setopt_str(curl_sys::CURLOPT_REFERER, &referer)
    }

    /// Sets the HTTP user-agent header
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_USERAGENT`.
    pub fn useragent(&mut self, useragent: &str) -> Result<(), Error> {
        let useragent = try!(CString::new(useragent));
        self.setopt_str(curl_sys::CURLOPT_USERAGENT, &useragent)
    }

    /// Add some headers to this HTTP request.
    ///
    /// If you add a header that is otherwise used internally, the value here
    /// takes precedence. If a header is added with no content (like `Accept:`)
    /// the internally the header will get disabled. To add a header with no
    /// content, use the form `MyHeader;` (not the trailing semicolon).
    ///
    /// Headers must not be CRLF terminated. Many replaced headers have common
    /// shortcuts which should be prefered.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_HTTPHEADER`
    ///
    /// # Examples
    ///
    /// ```
    /// use curl::easy::{Easy, List};
    ///
    /// let mut list = List::new();
    /// list.append("Foo: bar").unwrap();
    /// list.append("Bar: baz").unwrap();
    ///
    /// let mut handle = Easy::new();
    /// handle.url("https://www.rust-lang.org/").unwrap();
    /// handle.http_headers(list).unwrap();
    /// handle.perform().unwrap();
    /// ```
    pub fn http_headers(&mut self, list: List) -> Result<(), Error> {
        self.data.header_list = list;
        let ptr = self.data.header_list.raw;
        self.setopt_ptr(curl_sys::CURLOPT_HTTPHEADER, ptr as *const _)
    }

    // /// Add some headers to send to the HTTP proxy.
    // ///
    // /// This function is essentially the same as `http_headers`.
    // ///
    // /// By default this option is not set and corresponds to
    // /// `CURLOPT_PROXYHEADER`
    // pub fn proxy_headers(&mut self, list: &'a List) -> Result<(), Error> {
    //     self.setopt_ptr(curl_sys::CURLOPT_PROXYHEADER, list.raw as *const _)
    // }

    /// Set the contents of the HTTP Cookie header.
    ///
    /// Pass a string of the form `name=contents` for one cookie value or
    /// `name1=val1; name2=val2` for multiple values.
    ///
    /// Using this option multiple times will only make the latest string
    /// override the previous ones. This option will not enable the cookie
    /// engine, use `cookie_file` or `cookie_jar` to do that.
    ///
    /// By default this option is not set and corresponds to `CURLOPT_COOKIE`.
    pub fn cookie(&mut self, cookie: &str) -> Result<(), Error> {
        let cookie = try!(CString::new(cookie));
        self.setopt_str(curl_sys::CURLOPT_COOKIE, &cookie)
    }

    /// Set the file name to read cookies from.
    ///
    /// The cookie data can be in either the old Netscape / Mozilla cookie data
    /// format or just regular HTTP headers (Set-Cookie style) dumped to a file.
    ///
    /// This also enables the cookie engine, making libcurl parse and send
    /// cookies on subsequent requests with this handle.
    ///
    /// Given an empty or non-existing file or by passing the empty string ("")
    /// to this option, you can enable the cookie engine without reading any
    /// initial cookies.
    ///
    /// If you use this option multiple times, you just add more files to read.
    /// Subsequent files will add more cookies.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_COOKIEFILE`.
    pub fn cookie_file<P: AsRef<Path>>(&mut self, file: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_COOKIEFILE, file.as_ref())
    }

    /// Set the file name to store cookies to.
    ///
    /// This will make libcurl write all internally known cookies to the file
    /// when this handle is dropped. If no cookies are known, no file will be
    /// created. Specify "-" as filename to instead have the cookies written to
    /// stdout. Using this option also enables cookies for this session, so if
    /// you for example follow a location it will make matching cookies get sent
    /// accordingly.
    ///
    /// Note that libcurl doesn't read any cookies from the cookie jar. If you
    /// want to read cookies from a file, use `cookie_file`.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_COOKIEJAR`.
    pub fn cookie_jar<P: AsRef<Path>>(&mut self, file: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_COOKIEJAR, file.as_ref())
    }

    /// Start a new cookie session
    ///
    /// Marks this as a new cookie "session". It will force libcurl to ignore
    /// all cookies it is about to load that are "session cookies" from the
    /// previous session. By default, libcurl always stores and loads all
    /// cookies, independent if they are session cookies or not. Session cookies
    /// are cookies without expiry date and they are meant to be alive and
    /// existing for this "session" only.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_COOKIESESSION`.
    pub fn cookie_session(&mut self, session: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_COOKIESESSION, session as c_long)
    }

    /// Add to or manipulate cookies held in memory.
    ///
    /// Such a cookie can be either a single line in Netscape / Mozilla format
    /// or just regular HTTP-style header (Set-Cookie: ...) format. This will
    /// also enable the cookie engine. This adds that single cookie to the
    /// internal cookie store.
    ///
    /// Exercise caution if you are using this option and multiple transfers may
    /// occur. If you use the Set-Cookie format and don't specify a domain then
    /// the cookie is sent for any domain (even after redirects are followed)
    /// and cannot be modified by a server-set cookie. If a server sets a cookie
    /// of the same name (or maybe you've imported one) then both will be sent
    /// on a future transfer to that server, likely not what you intended.
    /// address these issues set a domain in Set-Cookie or use the Netscape
    /// format.
    ///
    /// Additionally, there are commands available that perform actions if you
    /// pass in these exact strings:
    ///
    /// * "ALL" - erases all cookies held in memory
    /// * "SESS" - erases all session cookies held in memory
    /// * "FLUSH" - write all known cookies to the specified cookie jar
    /// * "RELOAD" - reread all cookies from the cookie file
    ///
    /// By default this options corresponds to `CURLOPT_COOKIELIST`
    pub fn cookie_list(&mut self, cookie: &str) -> Result<(), Error> {
        let cookie = try!(CString::new(cookie));
        self.setopt_str(curl_sys::CURLOPT_COOKIELIST, &cookie)
    }

    /// Ask for a HTTP GET request.
    ///
    /// By default this option is `false` and corresponds to `CURLOPT_HTTPGET`.
    pub fn get(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_HTTPGET, enable as c_long)
    }

    // /// Ask for a HTTP GET request.
    // ///
    // /// By default this option is `false` and corresponds to `CURLOPT_HTTPGET`.
    // pub fn http_version(&mut self, vers: &str) -> Result<(), Error> {
    //     self.setopt_long(curl_sys::CURLOPT_HTTPGET, enable as c_long)
    // }

    /// Ignore the content-length header.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_IGNORE_CONTENT_LENGTH`.
    pub fn ignore_content_length(&mut self, ignore: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_IGNORE_CONTENT_LENGTH,
                         ignore as c_long)
    }

    /// Enable or disable HTTP content decoding.
    ///
    /// By default this option is `true` and corresponds to
    /// `CURLOPT_HTTP_CONTENT_DECODING`.
    pub fn http_content_decoding(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_HTTP_CONTENT_DECODING,
                         enable as c_long)
    }

    /// Enable or disable HTTP transfer decoding.
    ///
    /// By default this option is `true` and corresponds to
    /// `CURLOPT_HTTP_TRANSFER_DECODING`.
    pub fn http_transfer_decoding(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_HTTP_TRANSFER_DECODING,
                         enable as c_long)
    }

    // /// Timeout for the Expect: 100-continue response
    // ///
    // /// By default this option is 1s and corresponds to
    // /// `CURLOPT_EXPECT_100_TIMEOUT_MS`.
    // pub fn expect_100_timeout(&mut self, enable: bool) -> Result<(), Error> {
    //     self.setopt_long(curl_sys::CURLOPT_HTTP_TRANSFER_DECODING,
    //                      enable as c_long)
    // }

    // /// Wait for pipelining/multiplexing.
    // ///
    // /// Tells libcurl to prefer to wait for a connection to confirm or deny that
    // /// it can do pipelining or multiplexing before continuing.
    // ///
    // /// When about to perform a new transfer that allows pipelining or
    // /// multiplexing, libcurl will check for existing connections to re-use and
    // /// pipeline on. If no such connection exists it will immediately continue
    // /// and create a fresh new connection to use.
    // ///
    // /// By setting this option to `true` - having `pipeline` enabled for the
    // /// multi handle this transfer is associated with - libcurl will instead
    // /// wait for the connection to reveal if it is possible to
    // /// pipeline/multiplex on before it continues. This enables libcurl to much
    // /// better keep the number of connections to a minimum when using pipelining
    // /// or multiplexing protocols.
    // ///
    // /// The effect thus becomes that with this option set, libcurl prefers to
    // /// wait and re-use an existing connection for pipelining rather than the
    // /// opposite: prefer to open a new connection rather than waiting.
    // ///
    // /// The waiting time is as long as it takes for the connection to get up and
    // /// for libcurl to get the necessary response back that informs it about its
    // /// protocol and support level.
    // pub fn http_pipewait(&mut self, enable: bool) -> Result<(), Error> {
    // }


    // =========================================================================
    // Protocol Options

    /// Indicates the range that this request should retrieve.
    ///
    /// The string provided should be of the form `N-M` where either `N` or `M`
    /// can be left out. For HTTP transfers multiple ranges separated by commas
    /// are also accepted.
    ///
    /// By default this option is not set and corresponds to `CURLOPT_RANGE`.
    pub fn range(&mut self, range: &str) -> Result<(), Error> {
        let range = try!(CString::new(range));
        self.setopt_str(curl_sys::CURLOPT_RANGE, &range)
    }

    /// Set a point to resume transfer from
    ///
    /// Specify the offset in bytes you want the transfer to start from.
    ///
    /// By default this option is 0 and corresponds to
    /// `CURLOPT_RESUME_FROM_LARGE`.
    pub fn resume_from(&mut self, from: u64) -> Result<(), Error> {
        self.setopt_off_t(curl_sys::CURLOPT_RESUME_FROM_LARGE,
                          from as curl_sys::curl_off_t)
    }

    /// Set a custom request string
    ///
    /// Specifies that a custom request will be made (e.g. a custom HTTP
    /// method). This does not change how libcurl performs internally, just
    /// changes the string sent to the server.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_CUSTOMREQUEST`.
    pub fn custom_request(&mut self, request: &str) -> Result<(), Error> {
        let request = try!(CString::new(request));
        self.setopt_str(curl_sys::CURLOPT_CUSTOMREQUEST, &request)
    }

    /// Get the modification time of the remote resource
    ///
    /// If true, libcurl will attempt to get the modification time of the
    /// remote document in this operation. This requires that the remote server
    /// sends the time or replies to a time querying command. The `filetime`
    /// function can be used after a transfer to extract the received time (if
    /// any).
    ///
    /// By default this option is `false` and corresponds to `CURLOPT_FILETIME`
    pub fn fetch_filetime(&mut self, fetch: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_FILETIME, fetch as c_long)
    }

    /// Indicate whether to download the request without getting the body
    ///
    /// This is useful, for example, for doing a HEAD request.
    ///
    /// By default this option is `false` and corresponds to `CURLOPT_NOBODY`.
    pub fn nobody(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_NOBODY, enable as c_long)
    }

    /// Set the size of the input file to send off.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_INFILESIZE_LARGE`.
    pub fn in_filesize(&mut self, size: u64) -> Result<(), Error> {
        self.setopt_off_t(curl_sys::CURLOPT_INFILESIZE_LARGE,
                          size as curl_sys::curl_off_t)
    }

    /// Enable or disable data upload.
    ///
    /// This means that a PUT request will be made for HTTP and probably wants
    /// to be combined with the read callback as well as the `in_filesize`
    /// method.
    ///
    /// By default this option is `false` and corresponds to `CURLOPT_UPLOAD`.
    pub fn upload(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_UPLOAD, enable as c_long)
    }

    /// Configure the maximum file size to download.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_MAXFILESIZE_LARGE`.
    pub fn max_filesize(&mut self, size: u64) -> Result<(), Error> {
        self.setopt_off_t(curl_sys::CURLOPT_MAXFILESIZE_LARGE,
                          size as curl_sys::curl_off_t)
    }

    /// Selects a condition for a time request.
    ///
    /// This value indicates how the `time_value` option is interpreted.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_TIMECONDITION`.
    pub fn time_condition(&mut self, cond: TimeCondition) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_TIMECONDITION, cond as c_long)
    }

    /// Sets the time value for a conditional request.
    ///
    /// The value here should be the number of seconds elapsed since January 1,
    /// 1970. To pass how to interpret this value, use `time_condition`.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_TIMEVALUE`.
    pub fn time_value(&mut self, val: i64) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_TIMEVALUE, val as c_long)
    }

    // =========================================================================
    // Connection Options

    /// Set maximum time the request is allowed to take.
    ///
    /// Normally, name lookups can take a considerable time and limiting
    /// operations to less than a few minutes risk aborting perfectly normal
    /// operations.
    ///
    /// If libcurl is built to use the standard system name resolver, that
    /// portion of the transfer will still use full-second resolution for
    /// timeouts with a minimum timeout allowed of one second.
    ///
    /// In unix-like systems, this might cause signals to be used unless
    /// `nosignal` is set.
    ///
    /// Since this puts a hard limit for how long time a request is allowed to
    /// take, it has limited use in dynamic use cases with varying transfer
    /// times. You are then advised to explore `low_speed_limit`,
    /// `low_speed_time` or using `progress_function` to implement your own
    /// timeout logic.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_TIMEOUT_MS`.
    pub fn timeout(&mut self, timeout: Duration) -> Result<(), Error> {
        // TODO: checked arithmetic and casts
        // TODO: use CURLOPT_TIMEOUT if the timeout is too great
        let ms = timeout.as_secs() * 1000 +
                 (timeout.subsec_nanos() / 1_000_000) as u64;
        self.setopt_long(curl_sys::CURLOPT_TIMEOUT_MS, ms as c_long)

    }

    /// Set the low speed limit in bytes per second.
    ///
    /// This specifies the average transfer speed in bytes per second that the
    /// transfer should be below during `low_speed_time` for libcurl to consider
    /// it to be too slow and abort.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_LOW_SPEED_LIMIT`.
    pub fn low_speed_limit(&mut self, limit: u32) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_LOW_SPEED_LIMIT, limit as c_long)
    }

    /// Set the low speed time period.
    ///
    /// Specifies the window of time for which if the transfer rate is below
    /// `low_speed_limit` the request will be aborted.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_LOW_SPEED_TIME`.
    pub fn low_speed_time(&mut self, dur: Duration) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_LOW_SPEED_TIME,
                         dur.as_secs() as c_long)
    }

    /// Rate limit data upload speed
    ///
    /// If an upload exceeds this speed (counted in bytes per second) on
    /// cumulative average during the transfer, the transfer will pause to keep
    /// the average rate less than or equal to the parameter value.
    ///
    /// By default this option is not set (unlimited speed) and corresponds to
    /// `CURLOPT_MAX_SEND_SPEED_LARGE`.
    pub fn max_send_speed(&mut self, speed: u64) -> Result<(), Error> {
        self.setopt_off_t(curl_sys::CURLOPT_MAX_SEND_SPEED_LARGE,
                          speed as curl_sys::curl_off_t)
    }

    /// Rate limit data download speed
    ///
    /// If a download exceeds this speed (counted in bytes per second) on
    /// cumulative average during the transfer, the transfer will pause to keep
    /// the average rate less than or equal to the parameter value.
    ///
    /// By default this option is not set (unlimited speed) and corresponds to
    /// `CURLOPT_MAX_RECV_SPEED_LARGE`.
    pub fn max_recv_speed(&mut self, speed: u64) -> Result<(), Error> {
        self.setopt_off_t(curl_sys::CURLOPT_MAX_RECV_SPEED_LARGE,
                          speed as curl_sys::curl_off_t)
    }

    /// Set the maximum connection cache size.
    ///
    /// The set amount will be the maximum number of simultaneously open
    /// persistent connections that libcurl may cache in the pool associated
    /// with this handle. The default is 5, and there isn't much point in
    /// changing this value unless you are perfectly aware of how this works and
    /// changes libcurl's behaviour. This concerns connections using any of the
    /// protocols that support persistent connections.
    ///
    /// When reaching the maximum limit, curl closes the oldest one in the cache
    /// to prevent increasing the number of open connections.
    ///
    /// By default this option is set to 5 and corresponds to
    /// `CURLOPT_MAXCONNECTS`
    pub fn max_connects(&mut self, max: u32) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_MAXCONNECTS, max as c_long)
    }

    /// Force a new connection to be used.
    ///
    /// Makes the next transfer use a new (fresh) connection by force instead of
    /// trying to re-use an existing one. This option should be used with
    /// caution and only if you understand what it does as it may seriously
    /// impact performance.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_FRESH_CONNECT`.
    pub fn fresh_connect(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_FRESH_CONNECT, enable as c_long)
    }

    /// Make connection get closed at once after use.
    ///
    /// Makes libcurl explicitly close the connection when done with the
    /// transfer. Normally, libcurl keeps all connections alive when done with
    /// one transfer in case a succeeding one follows that can re-use them.
    /// This option should be used with caution and only if you understand what
    /// it does as it can seriously impact performance.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_FORBID_REUSE`.
    pub fn forbid_reuse(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_FORBID_REUSE, enable as c_long)
    }

    /// Timeout for the connect phase
    ///
    /// This is the maximum time that you allow the connection phase to the
    /// server to take. This only limits the connection phase, it has no impact
    /// once it has connected.
    ///
    /// By default this value is 300 seconds and corresponds to
    /// `CURLOPT_CONNECTTIMEOUT_MS`.
    pub fn connect_timeout(&mut self, timeout: Duration) -> Result<(), Error> {
        let ms = timeout.as_secs() * 1000 +
                 (timeout.subsec_nanos() / 1_000_000) as u64;
        self.setopt_long(curl_sys::CURLOPT_CONNECTTIMEOUT_MS, ms as c_long)
    }

    /// Specify which IP protocol version to use
    ///
    /// Allows an application to select what kind of IP addresses to use when
    /// resolving host names. This is only interesting when using host names
    /// that resolve addresses using more than one version of IP.
    ///
    /// By default this value is "any" and corresponds to `CURLOPT_IPRESOLVE`.
    pub fn ip_resolve(&mut self, resolve: IpResolve) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_IPRESOLVE, resolve as c_long)
    }

    /// Configure whether to stop when connected to target server
    ///
    /// When enabled it tells the library to perform all the required proxy
    /// authentication and connection setup, but no data transfer, and then
    /// return.
    ///
    /// The option can be used to simply test a connection to a server.
    ///
    /// By default this value is `false` and corresponds to
    /// `CURLOPT_CONNECT_ONLY`.
    pub fn connect_only(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_CONNECT_ONLY, enable as c_long)
    }

    // /// Set interface to speak DNS over.
    // ///
    // /// Set the name of the network interface that the DNS resolver should bind
    // /// to. This must be an interface name (not an address).
    // ///
    // /// By default this option is not set and corresponds to
    // /// `CURLOPT_DNS_INTERFACE`.
    // pub fn dns_interface(&mut self, interface: &str) -> Result<(), Error> {
    //     let interface = try!(CString::new(interface));
    //     self.setopt_str(curl_sys::CURLOPT_DNS_INTERFACE, &interface)
    // }
    //
    // /// IPv4 address to bind DNS resolves to
    // ///
    // /// Set the local IPv4 address that the resolver should bind to. The
    // /// argument should be of type char * and contain a single numerical IPv4
    // /// address as a string.
    // ///
    // /// By default this option is not set and corresponds to
    // /// `CURLOPT_DNS_LOCAL_IP4`.
    // pub fn dns_local_ip4(&mut self, ip: &str) -> Result<(), Error> {
    //     let ip = try!(CString::new(ip));
    //     self.setopt_str(curl_sys::CURLOPT_DNS_LOCAL_IP4, &ip)
    // }
    //
    // /// IPv6 address to bind DNS resolves to
    // ///
    // /// Set the local IPv6 address that the resolver should bind to. The
    // /// argument should be of type char * and contain a single numerical IPv6
    // /// address as a string.
    // ///
    // /// By default this option is not set and corresponds to
    // /// `CURLOPT_DNS_LOCAL_IP6`.
    // pub fn dns_local_ip6(&mut self, ip: &str) -> Result<(), Error> {
    //     let ip = try!(CString::new(ip));
    //     self.setopt_str(curl_sys::CURLOPT_DNS_LOCAL_IP6, &ip)
    // }
    //
    // /// Set preferred DNS servers.
    // ///
    // /// Provides a list of DNS servers to be used instead of the system default.
    // /// The format of the dns servers option is:
    // ///
    // /// ```text
    // /// host[:port],[host[:port]]...
    // /// ```
    // ///
    // /// By default this option is not set and corresponds to
    // /// `CURLOPT_DNS_SERVERS`.
    // pub fn dns_servers(&mut self, servers: &str) -> Result<(), Error> {
    //     let servers = try!(CString::new(servers));
    //     self.setopt_str(curl_sys::CURLOPT_DNS_SERVERS, &servers)
    // }

    // =========================================================================
    // SSL/Security Options

    /// Sets the SSL client certificate.
    ///
    /// The string should be the file name of your client certificate. The
    /// default format is "P12" on Secure Transport and "PEM" on other engines,
    /// and can be changed with `ssl_cert_type`.
    ///
    /// With NSS or Secure Transport, this can also be the nickname of the
    /// certificate you wish to authenticate with as it is named in the security
    /// database. If you want to use a file from the current directory, please
    /// precede it with "./" prefix, in order to avoid confusion with a
    /// nickname.
    ///
    /// When using a client certificate, you most likely also need to provide a
    /// private key with `ssl_key`.
    ///
    /// By default this option is not set and corresponds to `CURLOPT_SSLCERT`.
    pub fn ssl_cert<P: AsRef<Path>>(&mut self, cert: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_SSLCERT, cert.as_ref())
    }

    /// Specify type of the client SSL certificate.
    ///
    /// The string should be the format of your certificate. Supported formats
    /// are "PEM" and "DER", except with Secure Transport. OpenSSL (versions
    /// 0.9.3 and later) and Secure Transport (on iOS 5 or later, or OS X 10.7
    /// or later) also support "P12" for PKCS#12-encoded files.
    ///
    /// By default this option is "PEM" and corresponds to
    /// `CURLOPT_SSLCERTTYPE`.
    pub fn ssl_cert_type(&mut self, kind: &str) -> Result<(), Error> {
        let kind = try!(CString::new(kind));
        self.setopt_str(curl_sys::CURLOPT_SSLCERTTYPE, &kind)
    }

    /// Specify private keyfile for TLS and SSL client cert.
    ///
    /// The string should be the file name of your private key. The default
    /// format is "PEM" and can be changed with `ssl_key_type`.
    ///
    /// (iOS and Mac OS X only) This option is ignored if curl was built against
    /// Secure Transport. Secure Transport expects the private key to be already
    /// present in the keychain or PKCS#12 file containing the certificate.
    ///
    /// By default this option is not set and corresponds to `CURLOPT_SSLKEY`.
    pub fn ssl_key<P: AsRef<Path>>(&mut self, key: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_SSLKEY, key.as_ref())
    }

    /// Set type of the private key file.
    ///
    /// The string should be the format of your private key. Supported formats
    /// are "PEM", "DER" and "ENG".
    ///
    /// The format "ENG" enables you to load the private key from a crypto
    /// engine. In this case `ssl_key` is used as an identifier passed to
    /// the engine. You have to set the crypto engine with `ssl_engine`.
    /// "DER" format key file currently does not work because of a bug in
    /// OpenSSL.
    ///
    /// By default this option is "PEM" and corresponds to
    /// `CURLOPT_SSLKEYTYPE`.
    pub fn ssl_key_type(&mut self, kind: &str) -> Result<(), Error> {
        let kind = try!(CString::new(kind));
        self.setopt_str(curl_sys::CURLOPT_SSLKEYTYPE, &kind)
    }

    /// Set passphrase to private key.
    ///
    /// This will be used as the password required to use the `ssl_key`.
    /// You never needed a pass phrase to load a certificate but you need one to
    /// load your private key.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_KEYPASSWD`.
    pub fn key_password(&mut self, password: &str) -> Result<(), Error> {
        let password = try!(CString::new(password));
        self.setopt_str(curl_sys::CURLOPT_KEYPASSWD, &password)
    }

    /// Set the SSL engine identifier.
    ///
    /// This will be used as the identifier for the crypto engine you want to
    /// use for your private key.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_SSLENGINE`.
    pub fn ssl_engine(&mut self, engine: &str) -> Result<(), Error> {
        let engine = try!(CString::new(engine));
        self.setopt_str(curl_sys::CURLOPT_SSLENGINE, &engine)
    }

    /// Make this handle's SSL engine the default.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_SSLENGINE_DEFAULT`.
    pub fn ssl_engine_default(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_SSLENGINE_DEFAULT, enable as c_long)
    }

    // /// Enable TLS false start.
    // ///
    // /// This option determines whether libcurl should use false start during the
    // /// TLS handshake. False start is a mode where a TLS client will start
    // /// sending application data before verifying the server's Finished message,
    // /// thus saving a round trip when performing a full handshake.
    // ///
    // /// By default this option is not set and corresponds to
    // /// `CURLOPT_SSL_FALSESTARTE`.
    // pub fn ssl_false_start(&mut self, enable: bool) -> Result<(), Error> {
    //     self.setopt_long(curl_sys::CURLOPT_SSLENGINE_DEFAULT, enable as c_long)
    // }

    /// Set preferred TLS/SSL version.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_SSLVERSION`.
    pub fn ssl_version(&mut self, version: SslVersion) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_SSLVERSION, version as c_long)
    }

    /// Verify the certificate's name against host.
    ///
    /// This should be disabled with great caution! It basically disables the
    /// security features of SSL if it is disabled.
    ///
    /// By default this option is set to `true` and corresponds to
    /// `CURLOPT_SSL_VERIFYHOST`.
    pub fn ssl_verify_host(&mut self, verify: bool) -> Result<(), Error> {
        let val = if verify {2} else {0};
        self.setopt_long(curl_sys::CURLOPT_SSL_VERIFYHOST, val)
    }

    /// Verify the peer's SSL certificate.
    ///
    /// This should be disabled with great caution! It basically disables the
    /// security features of SSL if it is disabled.
    ///
    /// By default this option is set to `true` and corresponds to
    /// `CURLOPT_SSL_VERIFYPEER`.
    pub fn ssl_verify_peer(&mut self, verify: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_SSL_VERIFYPEER, verify as c_long)
    }

    // /// Verify the certificate's status.
    // ///
    // /// This option determines whether libcurl verifies the status of the server
    // /// cert using the "Certificate Status Request" TLS extension (aka. OCSP
    // /// stapling).
    // ///
    // /// By default this option is set to `false` and corresponds to
    // /// `CURLOPT_SSL_VERIFYSTATUS`.
    // pub fn ssl_verify_status(&mut self, verify: bool) -> Result<(), Error> {
    //     self.setopt_long(curl_sys::CURLOPT_SSL_VERIFYSTATUS, verify as c_long)
    // }

    /// Specify the path to Certificate Authority (CA) bundle
    ///
    /// The file referenced should hold one or more certificates to verify the
    /// peer with.
    ///
    /// This option is by default set to the system path where libcurl's cacert
    /// bundle is assumed to be stored, as established at build time.
    ///
    /// If curl is built against the NSS SSL library, the NSS PEM PKCS#11 module
    /// (libnsspem.so) needs to be available for this option to work properly.
    ///
    /// By default this option is the system defaults, and corresponds to
    /// `CURLOPT_CAINFO`.
    pub fn cainfo<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_CAINFO, path.as_ref())
    }

    /// Set the issuer SSL certificate filename
    ///
    /// Specifies a file holding a CA certificate in PEM format. If the option
    /// is set, an additional check against the peer certificate is performed to
    /// verify the issuer is indeed the one associated with the certificate
    /// provided by the option. This additional check is useful in multi-level
    /// PKI where one needs to enforce that the peer certificate is from a
    /// specific branch of the tree.
    ///
    /// This option makes sense only when used in combination with the
    /// `ssl_verify_peer` option. Otherwise, the result of the check is not
    /// considered as failure.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_ISSUERCERT`.
    pub fn issuer_cert<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_ISSUERCERT, path.as_ref())
    }

    /// Specify directory holding CA certificates
    ///
    /// Names a directory holding multiple CA certificates to verify the peer
    /// with. If libcurl is built against OpenSSL, the certificate directory
    /// must be prepared using the openssl c_rehash utility. This makes sense
    /// only when used in combination with the `ssl_verify_peer` option.
    ///
    /// By default this option is not set and corresponds to `CURLOPT_CAPATH`.
    pub fn capath<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_CAPATH, path.as_ref())
    }

    /// Specify a Certificate Revocation List file
    ///
    /// Names a file with the concatenation of CRL (in PEM format) to use in the
    /// certificate validation that occurs during the SSL exchange.
    ///
    /// When curl is built to use NSS or GnuTLS, there is no way to influence
    /// the use of CRL passed to help in the verification process. When libcurl
    /// is built with OpenSSL support, X509_V_FLAG_CRL_CHECK and
    /// X509_V_FLAG_CRL_CHECK_ALL are both set, requiring CRL check against all
    /// the elements of the certificate chain if a CRL file is passed.
    ///
    /// This option makes sense only when used in combination with the
    /// `ssl_verify_peer` option.
    ///
    /// A specific error code (`is_ssl_crl_badfile`) is defined with the
    /// option. It is returned when the SSL exchange fails because the CRL file
    /// cannot be loaded. A failure in certificate verification due to a
    /// revocation information found in the CRL does not trigger this specific
    /// error.
    ///
    /// By default this option is not set and corresponds to `CURLOPT_CRLFILE`.
    pub fn crlfile<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_CRLFILE, path.as_ref())
    }

    /// Request SSL certificate information
    ///
    /// Enable libcurl's certificate chain info gatherer. With this enabled,
    /// libcurl will extract lots of information and data about the certificates
    /// in the certificate chain used in the SSL connection.
    ///
    /// By default this option is `false` and corresponds to
    /// `CURLOPT_CERTINFO`.
    pub fn certinfo(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_CERTINFO, enable as c_long)
    }

    // /// Set pinned public key.
    // ///
    // /// Pass a pointer to a zero terminated string as parameter. The string can
    // /// be the file name of your pinned public key. The file format expected is
    // /// "PEM" or "DER". The string can also be any number of base64 encoded
    // /// sha256 hashes preceded by "sha256//" and separated by ";"
    // ///
    // /// When negotiating a TLS or SSL connection, the server sends a certificate
    // /// indicating its identity. A public key is extracted from this certificate
    // /// and if it does not exactly match the public key provided to this option,
    // /// curl will abort the connection before sending or receiving any data.
    // ///
    // /// By default this option is not set and corresponds to
    // /// `CURLOPT_PINNEDPUBLICKEY`.
    // pub fn pinned_public_key(&mut self, enable: bool) -> Result<(), Error> {
    //     self.setopt_long(curl_sys::CURLOPT_CERTINFO, enable as c_long)
    // }

    /// Specify a source for random data
    ///
    /// The file will be used to read from to seed the random engine for SSL and
    /// more.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_RANDOM_FILE`.
    pub fn random_file<P: AsRef<Path>>(&mut self, p: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_RANDOM_FILE, p.as_ref())
    }

    /// Specify EGD socket path.
    ///
    /// Indicates the path name to the Entropy Gathering Daemon socket. It will
    /// be used to seed the random engine for SSL.
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_EGDSOCKET`.
    pub fn egd_socket<P: AsRef<Path>>(&mut self, p: P) -> Result<(), Error> {
        self.setopt_path(curl_sys::CURLOPT_EGDSOCKET, p.as_ref())
    }

    /// Specify ciphers to use for TLS.
    ///
    /// Holds the list of ciphers to use for the SSL connection. The list must
    /// be syntactically correct, it consists of one or more cipher strings
    /// separated by colons. Commas or spaces are also acceptable separators
    /// but colons are normally used, !, - and + can be used as operators.
    ///
    /// For OpenSSL and GnuTLS valid examples of cipher lists include 'RC4-SHA',
    /// ´SHA1+DES´, 'TLSv1' and 'DEFAULT'. The default list is normally set when
    /// you compile OpenSSL.
    ///
    /// You'll find more details about cipher lists on this URL:
    ///
    /// https://www.openssl.org/docs/apps/ciphers.html
    ///
    /// For NSS, valid examples of cipher lists include 'rsa_rc4_128_md5',
    /// ´rsa_aes_128_sha´, etc. With NSS you don't add/remove ciphers. If one
    /// uses this option then all known ciphers are disabled and only those
    /// passed in are enabled.
    ///
    /// You'll find more details about the NSS cipher lists on this URL:
    ///
    /// http://git.fedorahosted.org/cgit/mod_nss.git/plain/docs/mod_nss.html#Directives
    ///
    /// By default this option is not set and corresponds to
    /// `CURLOPT_SSL_CIPHER_LIST`.
    pub fn ssl_cipher_list(&mut self, ciphers: &str) -> Result<(), Error> {
        let ciphers = try!(CString::new(ciphers));
        self.setopt_str(curl_sys::CURLOPT_SSL_CIPHER_LIST, &ciphers)
    }

    /// Enable or disable use of the SSL session-ID cache
    ///
    /// By default all transfers are done using the cache enabled. While nothing
    /// ever should get hurt by attempting to reuse SSL session-IDs, there seem
    /// to be or have been broken SSL implementations in the wild that may
    /// require you to disable this in order for you to succeed.
    ///
    /// This corresponds to the `CURLOPT_SSL_SESSIONID_CACHE` option.
    pub fn ssl_sessionid_cache(&mut self, enable: bool) -> Result<(), Error> {
        self.setopt_long(curl_sys::CURLOPT_SSL_SESSIONID_CACHE,
                         enable as c_long)
    }

    // /// Stores a private pointer-sized piece of data.
    // ///
    // /// This can be retrieved through the `private` function and otherwise
    // /// libcurl does not tamper with this value. This corresponds to
    // /// `CURLOPT_PRIVATE` and defaults to 0.
    // pub fn set_private(&mut self, private: usize) -> Result<(), Error> {
    //     self.setopt_ptr(curl_sys::CURLOPT_PRIVATE, private as *const _)
    // }

    // /// Fetches this handle's private pointer-sized piece of data.
    // ///
    // /// This corresponds to
    // /// `CURLINFO_PRIVATE` and defaults to 0.
    // pub fn private(&mut self) -> Result<usize, Error> {
    //     self.getopt_ptr(curl_sys::CURLINFO_PRIVATE).map(|p| p as usize)
    // }

    // =========================================================================
    // getters

    /// Get the last used URL
    ///
    /// In cases when you've asked libcurl to follow redirects, it may
    /// not be the same value you set with `url`.
    ///
    /// This methods corresponds to the `CURLINFO_EFFECTIVE_URL` option.
    ///
    /// Returns `Ok(None)` if no effective url is listed or `Err` if an error
    /// happens or the underlying bytes aren't valid utf-8.
    pub fn effective_url(&mut self) -> Result<Option<&str>, Error> {
        self.getopt_str(curl_sys::CURLINFO_EFFECTIVE_URL)
    }

    /// Get the last used URL, in bytes
    ///
    /// In cases when you've asked libcurl to follow redirects, it may
    /// not be the same value you set with `url`.
    ///
    /// This methods corresponds to the `CURLINFO_EFFECTIVE_URL` option.
    ///
    /// Returns `Ok(None)` if no effective url is listed or `Err` if an error
    /// happens or the underlying bytes aren't valid utf-8.
    pub fn effective_url_bytes(&mut self) -> Result<Option<&[u8]>, Error> {
        self.getopt_bytes(curl_sys::CURLINFO_EFFECTIVE_URL)
    }

    /// Get the last response code
    ///
    /// The stored value will be zero if no server response code has been
    /// received. Note that a proxy's CONNECT response should be read with
    /// `http_connectcode` and not this.
    ///
    /// Corresponds to `CURLINFO_RESPONSE_CODE` and returns an error if this
    /// option is not supported.
    pub fn response_code(&mut self) -> Result<u32, Error> {
        self.getopt_long(curl_sys::CURLINFO_RESPONSE_CODE).map(|c| c as u32)
    }

    /// Get the CONNECT response code
    ///
    /// Returns the last received HTTP proxy response code to a CONNECT request.
    /// The returned value will be zero if no such response code was available.
    ///
    /// Corresponds to `CURLINFO_HTTP_CONNECTCODE` and returns an error if this
    /// option is not supported.
    pub fn http_connectcode(&mut self) -> Result<u32, Error> {
        self.getopt_long(curl_sys::CURLINFO_HTTP_CONNECTCODE).map(|c| c as u32)
    }

    /// Get the remote time of the retrieved document
    ///
    /// Returns the remote time of the retrieved document (in number of seconds
    /// since 1 Jan 1970 in the GMT/UTC time zone). If you get `None`, it can be
    /// because of many reasons (it might be unknown, the server might hide it
    /// or the server doesn't support the command that tells document time etc)
    /// and the time of the document is unknown.
    ///
    /// Note that you must tell the server to collect this information before
    /// the transfer is made, by using the `filetime` method to
    /// or you will unconditionally get a `None` back.
    ///
    /// This corresponds to `CURLINFO_FILETIME` and may return an error if the
    /// option is not supported
    pub fn filetime(&mut self) -> Result<Option<i64>, Error> {
        self.getopt_long(curl_sys::CURLINFO_FILETIME).map(|r| {
            if r == -1 {
                None
            } else {
                Some(r as i64)
            }
        })
    }

    /// Get the number of redirects
    ///
    /// Corresponds to `CURLINFO_REDIRECT_COUNT` and may return an error if the
    /// option isn't supported.
    pub fn redirect_count(&mut self) -> Result<u32, Error> {
        self.getopt_long(curl_sys::CURLINFO_REDIRECT_COUNT).map(|c| c as u32)
    }

    /// Get the URL a redirect would go to
    ///
    /// Returns the URL a redirect would take you to if you would enable
    /// `follow_location`. This can come very handy if you think using the
    /// built-in libcurl redirect logic isn't good enough for you but you would
    /// still prefer to avoid implementing all the magic of figuring out the new
    /// URL.
    ///
    /// Corresponds to `CURLINFO_REDIRECT_URL` and may return an error if the
    /// url isn't valid utf-8 or an error happens.
    pub fn redirect_url(&mut self) -> Result<Option<&str>, Error> {
        self.getopt_str(curl_sys::CURLINFO_REDIRECT_URL)
    }

    /// Get the URL a redirect would go to, in bytes
    ///
    /// Returns the URL a redirect would take you to if you would enable
    /// `follow_location`. This can come very handy if you think using the
    /// built-in libcurl redirect logic isn't good enough for you but you would
    /// still prefer to avoid implementing all the magic of figuring out the new
    /// URL.
    ///
    /// Corresponds to `CURLINFO_REDIRECT_URL` and may return an error.
    pub fn redirect_url_bytes(&mut self) -> Result<Option<&[u8]>, Error> {
        self.getopt_bytes(curl_sys::CURLINFO_REDIRECT_URL)
    }

    /// Get size of retrieved headers
    ///
    /// Corresponds to `CURLINFO_HEADER_SIZE` and may return an error if the
    /// option isn't supported.
    pub fn header_size(&mut self) -> Result<u64, Error> {
        self.getopt_long(curl_sys::CURLINFO_HEADER_SIZE).map(|c| c as u64)
    }

    /// Get size of sent request.
    ///
    /// Corresponds to `CURLINFO_REQUEST_SIZE` and may return an error if the
    /// option isn't supported.
    pub fn request_size(&mut self) -> Result<u64, Error> {
        self.getopt_long(curl_sys::CURLINFO_REQUEST_SIZE).map(|c| c as u64)
    }

    /// Get Content-Type
    ///
    /// Returns the content-type of the downloaded object. This is the value
    /// read from the Content-Type: field.  If you get `None`, it means that the
    /// server didn't send a valid Content-Type header or that the protocol
    /// used doesn't support this.
    ///
    /// Corresponds to `CURLINFO_CONTENT_TYPE` and may return an error if the
    /// option isn't supported.
    pub fn content_type(&mut self) -> Result<Option<&str>, Error> {
        self.getopt_str(curl_sys::CURLINFO_CONTENT_TYPE)
    }

    /// Get Content-Type, in bytes
    ///
    /// Returns the content-type of the downloaded object. This is the value
    /// read from the Content-Type: field.  If you get `None`, it means that the
    /// server didn't send a valid Content-Type header or that the protocol
    /// used doesn't support this.
    ///
    /// Corresponds to `CURLINFO_CONTENT_TYPE` and may return an error if the
    /// option isn't supported.
    pub fn content_type_bytes(&mut self) -> Result<Option<&[u8]>, Error> {
        self.getopt_bytes(curl_sys::CURLINFO_CONTENT_TYPE)
    }

    /// Get errno number from last connect failure.
    ///
    /// Note that the value is only set on failure, it is not reset upon a
    /// successful operation. The number is OS and system specific.
    ///
    /// Corresponds to `CURLINFO_OS_ERRNO` and may return an error if the
    /// option isn't supported.
    pub fn os_errno(&mut self) -> Result<i32, Error> {
        self.getopt_long(curl_sys::CURLINFO_OS_ERRNO).map(|c| c as i32)
    }

    /// Get IP address of last connection.
    ///
    /// Returns a string holding the IP address of the most recent connection
    /// done with this curl handle. This string may be IPv6 when that is
    /// enabled.
    ///
    /// Corresponds to `CURLINFO_PRIMARY_IP` and may return an error if the
    /// option isn't supported.
    pub fn primary_ip(&mut self) -> Result<Option<&str>, Error> {
        self.getopt_str(curl_sys::CURLINFO_PRIMARY_IP)
    }

    /// Get the latest destination port number
    ///
    /// Corresponds to `CURLINFO_PRIMARY_PORT` and may return an error if the
    /// option isn't supported.
    pub fn primary_port(&mut self) -> Result<u16, Error> {
        self.getopt_long(curl_sys::CURLINFO_PRIMARY_PORT).map(|c| c as u16)
    }

    /// Get local IP address of last connection
    ///
    /// Returns a string holding the IP address of the local end of most recent
    /// connection done with this curl handle. This string may be IPv6 when that
    /// is enabled.
    ///
    /// Corresponds to `CURLINFO_LOCAL_IP` and may return an error if the
    /// option isn't supported.
    pub fn local_ip(&mut self) -> Result<Option<&str>, Error> {
        self.getopt_str(curl_sys::CURLINFO_LOCAL_IP)
    }

    /// Get the latest local port number
    ///
    /// Corresponds to `CURLINFO_LOCAL_PORT` and may return an error if the
    /// option isn't supported.
    pub fn local_port(&mut self) -> Result<u16, Error> {
        self.getopt_long(curl_sys::CURLINFO_LOCAL_PORT).map(|c| c as u16)
    }

    /// Get all known cookies
    ///
    /// Returns a linked-list of all cookies cURL knows (expired ones, too).
    ///
    /// Corresponds to the `CURLINFO_COOKIELIST` option and may return an error
    /// if the option isn't supported.
    pub fn cookies(&mut self) -> Result<List, Error> {
        unsafe {
            let mut list = 0 as *mut _;
            try!(cvt(curl_sys::curl_easy_getinfo(self.handle,
                                                 curl_sys::CURLINFO_COOKIELIST,
                                                 &mut list)));
            Ok(List { raw: list })
        }
    }

    // =========================================================================
    // Other methods

    /// After options have been set, this will perform the transfer described by
    /// the options.
    ///
    /// This performs the request in a synchronous fashion. This can be used
    /// multiple times for one easy handle and libcurl will attempt to re-use
    /// the same connection for all transfers.
    ///
    /// This method will preserve all options configured in this handle for the
    /// next request, and if that is not desired then the options can be
    /// manually reset or the `reset` method can be called.
    pub fn perform(&mut self) -> Result<(), Error> {
        let ret = unsafe {
            cvt(curl_sys::curl_easy_perform(self.handle))
        };
        panic::propagate();
        return ret
    }

    /// URL encodes a string `s`
    pub fn url_encode(&mut self, s: &[u8]) -> String {
        if s.len() == 0 {
            return String::new()
        }
        unsafe {
            let p = curl_sys::curl_easy_escape(self.handle,
                                               s.as_ptr() as *const _,
                                               s.len() as c_int);
            assert!(!p.is_null());
            let ret = str::from_utf8(CStr::from_ptr(p).to_bytes()).unwrap();
            let ret = String::from(ret);
            curl_sys::curl_free(p as *mut _);
            return ret
        }
    }

    /// URL decodes a string `s`, returning `None` if it fails
    pub fn url_decode(&mut self, s: &str) -> Vec<u8> {
        if s.len() == 0 {
            return Vec::new();
        }

        // Work around https://curl.haxx.se/docs/adv_20130622.html, a bug where
        // if the last few characters are a bad escape then curl will have a
        // buffer overrun.
        let mut iter = s.chars().rev();
        let orig_len = s.len();
        let mut data;
        let mut s = s;
        if iter.next() == Some('%') ||
           iter.next() == Some('%') ||
           iter.next() == Some('%') {
            data = s.to_string();
            data.push(0u8 as char);
            s = &data[..];
        }
        unsafe {
            let mut len = 0;
            let p = curl_sys::curl_easy_unescape(self.handle,
                                                 s.as_ptr() as *const _,
                                                 orig_len as c_int,
                                                 &mut len);
            assert!(!p.is_null());
            let slice = slice::from_raw_parts(p as *const u8, len as usize);
            let ret = slice.to_vec();
            curl_sys::curl_free(p as *mut _);
            return ret
        }
    }

    // TODO: I don't think this is safe, you can drop this which has all the
    //       callback data and then the next is use-after-free
    //
    // /// Attempts to clone this handle, returning a new session handle with the
    // /// same options set for this handle.
    // ///
    // /// Internal state info and things like persistent connections ccannot be
    // /// transferred.
    // ///
    // /// # Errors
    // ///
    // /// If a new handle could not be allocated or another error happens, `None`
    // /// is returned.
    // pub fn try_clone<'b>(&mut self) -> Option<Easy<'b>> {
    //     unsafe {
    //         let handle = curl_sys::curl_easy_duphandle(self.handle);
    //         if handle.is_null() {
    //             None
    //         } else {
    //             Some(Easy {
    //                 handle: handle,
    //                 data: blank_data(),
    //                 _marker: marker::PhantomData,
    //             })
    //         }
    //     }
    // }

    /// Re-initializes this handle to the default values.
    ///
    /// This puts the handle to the same state as it was in when it was just
    /// created. This does, however, keep live connections, the session id
    /// cache, the dns cache, and cookies.
    pub fn reset(&mut self) {
        unsafe {
            curl_sys::curl_easy_reset(self.handle);
        }
        default_configure(self);
    }

    /// Receives data from a connected socket.
    ///
    /// Only useful after a successful `perform` with the `connect_only` option
    /// set as well.
    pub fn recv(&mut self, data: &mut [u8]) -> Result<usize, Error> {
        unsafe {
            let mut n = 0;
            let r = curl_sys::curl_easy_recv(self.handle,
                                             data.as_mut_ptr() as *mut _,
                                             data.len(),
                                             &mut n);
            if r == curl_sys::CURLE_OK {
                Ok(n)
            } else {
                Err(Error::new(r))
            }
        }
    }

    /// Sends data over the connected socket.
    ///
    /// Only useful after a successful `perform` with the `connect_only` option
    /// set as well.
    pub fn send(&mut self, data: &[u8]) -> Result<usize, Error> {
        unsafe {
            let mut n = 0;
            cvt(curl_sys::curl_easy_send(self.handle,
                                         data.as_ptr() as *const _,
                                         data.len(),
                                         &mut n))
                .map(|()| n)
        }
    }

    /// Get a pointer to the raw underlying CURL handle.
    pub fn raw(&self) -> *mut curl_sys::CURL {
        self.handle
    }

    #[cfg(unix)]
    fn setopt_path(&mut self,
                   opt: curl_sys::CURLoption,
                   val: &Path) -> Result<(), Error> {
        use std::os::unix::prelude::*;
        let s = try!(CString::new(val.as_os_str().as_bytes()));
        self.setopt_str(opt, &s)
    }

    #[cfg(windows)]
    fn setopt_path(&mut self,
                   opt: curl_sys::CURLoption,
                   val: &Path) -> Result<(), Error> {
        match val.to_str() {
            Some(s) => self.setopt_str(opt, &try!(CString::new(s))),
            None => Err(Error::new(curl_sys::CURLE_CONV_FAILED)),
        }
    }

    fn setopt_long(&mut self,
                   opt: curl_sys::CURLoption,
                   val: c_long) -> Result<(), Error> {
        unsafe {
            cvt(curl_sys::curl_easy_setopt(self.handle, opt, val))
        }
    }

    fn setopt_str(&mut self,
                  opt: curl_sys::CURLoption,
                  val: &CStr) -> Result<(), Error> {
        self.setopt_ptr(opt, val.as_ptr())
    }

    fn setopt_ptr(&mut self,
                  opt: curl_sys::CURLoption,
                  val: *const c_char) -> Result<(), Error> {
        unsafe {
            cvt(curl_sys::curl_easy_setopt(self.handle, opt, val))
        }
    }

    fn setopt_off_t(&mut self,
                    opt: curl_sys::CURLoption,
                    val: curl_sys::curl_off_t) -> Result<(), Error> {
        unsafe {
            cvt(curl_sys::curl_easy_setopt(self.handle, opt, val))
        }
    }

    fn getopt_bytes(&mut self, opt: curl_sys::CURLINFO)
                    -> Result<Option<&[u8]>, Error> {
        unsafe {
            let p = try!(self.getopt_ptr(opt));
            if p.is_null() {
                Ok(None)
            } else {
                Ok(Some(CStr::from_ptr(p).to_bytes()))
            }
        }
    }

    fn getopt_ptr(&mut self, opt: curl_sys::CURLINFO)
                  -> Result<*const c_char, Error> {
        unsafe {
            let mut p = 0 as *const c_char;
            try!(cvt(curl_sys::curl_easy_getinfo(self.handle, opt, &mut p)));
            Ok(p)
        }
    }

    fn getopt_str(&mut self, opt: curl_sys::CURLINFO)
                    -> Result<Option<&str>, Error> {
        match self.getopt_bytes(opt) {
            Ok(None) => Ok(None),
            Err(e) => Err(e),
            Ok(Some(bytes)) => {
                match str::from_utf8(bytes) {
                    Ok(s) => Ok(Some(s)),
                    Err(_) => Err(Error::new(curl_sys::CURLE_CONV_FAILED)),
                }
            }
        }
    }

    fn getopt_long(&mut self, opt: curl_sys::CURLINFO) -> Result<c_long, Error> {
        unsafe {
            let mut p = 0;
            try!(cvt(curl_sys::curl_easy_getinfo(self.handle, opt, &mut p)));
            Ok(p)
        }
    }
}

fn blank_data<'a>() -> Box<EasyData<'a>> {
    Box::new(EasyData {
        write: Box::new(|_| panic!()),
        read: Box::new(|_| panic!()),
        seek: Box::new(|_| panic!()),
        debug: Box::new(|_, _| panic!()),
        header: Box::new(|_| panic!()),
        progress: Box::new(|_, _, _, _| panic!()),
        header_list: List::new(),
    })
}

fn default_configure(handle: &mut Easy) {
    let _ = handle.signal(false);
    ssl_configure(handle);
}

#[cfg(all(unix, not(target_os = "macos")))]
fn ssl_configure(handle: &mut Easy) {
    let probe = ::openssl_sys::probe::probe();
    if let Some(ref path) = probe.cert_file {
        let _ = handle.cainfo(path);
    }
    if let Some(ref path) = probe.cert_dir {
        let _ = handle.capath(path);
    }
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn ssl_configure(_handle: &mut Easy) {}

impl<'a> Drop for Easy<'a> {
    fn drop(&mut self) {
        unsafe {
            curl_sys::curl_easy_cleanup(self.handle);
        }
    }
}

impl List {
    /// Creates a new empty list of strings.
    pub fn new() -> List {
        List { raw: 0 as *mut _ }
    }

    /// Appends some data into this list.
    pub fn append(&mut self, data: &str) -> Result<(), Error> {
        let data = try!(CString::new(data));
        unsafe {
            let raw = curl_sys::curl_slist_append(self.raw, data.as_ptr());
            assert!(!raw.is_null());
            self.raw = raw;
            Ok(())
        }
    }

    /// Returns an iterator over the nodes in this list.
    pub fn iter(&self) -> Iter {
        Iter { _me: self, cur: self.raw }
    }
}

impl Drop for List {
    fn drop(&mut self) {
        unsafe {
            curl_sys::curl_slist_free_all(self.raw)
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.cur.is_null() {
            return None
        }

        unsafe {
            let ret = Some(CStr::from_ptr((*self.cur).data).to_bytes());
            self.cur = (*self.cur).next;
            return ret
        }
    }
}