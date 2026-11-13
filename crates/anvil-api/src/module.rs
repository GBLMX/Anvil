/// Stable module identity in `<vendor>.<name>` form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleId(&'static str);

impl ModuleId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }

    pub fn is_valid(self) -> bool {
        let Some((vendor, name)) = self.0.split_once('.') else {
            return false;
        };
        !vendor.is_empty()
            && !name.is_empty()
            && !name.contains('.')
            && vendor.bytes().chain(name.bytes()).all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
    }
}
