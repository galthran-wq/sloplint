//! PyPI distribution-name normalization and the import→distribution alias table.

/// PEP 503 normalization of a distribution name: lowercase, with runs of `-`, `_`, and `.`
/// collapsed to a single `-`. So `Foo.Bar_baz` and `foo-bar-baz` compare equal.
pub fn normalize_dist(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch == '-' || ch == '_' || ch == '.' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        } else {
            out.extend(ch.to_lowercase());
            prev_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

/// Distribution names that ship under an import name different from the distribution name.
/// Returns the normalized distribution name(s) an import maps to, for the common mismatches
/// where `import foo` does *not* come from a distribution called `foo`. The caller always
/// also tests the normalized import name itself, so this only lists the exceptions.
pub(crate) fn distribution_aliases(import_top: &str) -> &'static [&'static str] {
    match import_top {
        "cv2" => &["opencv-python"],
        "PIL" => &["pillow"],
        "yaml" => &["pyyaml"],
        "bs4" => &["beautifulsoup4"],
        "sklearn" => &["scikit-learn"],
        "skimage" => &["scikit-image"],
        "dateutil" => &["python-dateutil"],
        "dotenv" => &["python-dotenv"],
        "jose" => &["python-jose"],
        "slugify" => &["python-slugify"],
        "magic" => &["python-magic"],
        "docx" => &["python-docx"],
        "pptx" => &["python-pptx"],
        "attr" => &["attrs"],
        "jwt" => &["pyjwt"],
        "nacl" => &["pynacl"],
        "zmq" => &["pyzmq"],
        "serial" => &["pyserial"],
        "usb" => &["pyusb"],
        "OpenSSL" => &["pyopenssl"],
        "Crypto" => &["pycryptodome", "pycrypto"],
        "Cryptodome" => &["pycryptodomex"],
        "fitz" => &["pymupdf"],
        "bson" | "gridfs" => &["pymongo"],
        "psycopg2" => &["psycopg2-binary"],
        "grpc" => &["grpcio"],
        "mpl_toolkits" => &["matplotlib"],
        _ => &[],
    }
}

/// Top-level modules that ship with essentially every pip/virtualenv environment but are not
/// part of `sys.stdlib_module_names` and are routinely imported without being declared
/// (version lookups, `conftest.py`, build/entry-point helpers). Treating them as always
/// available keeps the conservative bias — flagging them would be a false positive on code
/// that works fine on a clean install.
pub(crate) fn is_always_available(module: &str) -> bool {
    matches!(
        module,
        "setuptools" | "pkg_resources" | "pip" | "wheel" | "_distutils_hack"
    )
}
