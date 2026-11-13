use std::sync::Mutex;

/// The system call surface Secure Keyboard Entry needs; swapped out in tests.
pub trait SecureInputBackend {
    /// Takes one system-wide secure input reference (`EnableSecureEventInput`).
    fn enable(&mut self);
    /// Releases one system-wide secure input reference (`DisableSecureEventInput`).
    fn disable(&mut self);
}

/// Carbon's `EnableSecureEventInput`/`DisableSecureEventInput` on macOS; a no-op elsewhere.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SystemSecureInput;

#[cfg(target_os = "macos")]
mod carbon {
    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        pub(super) fn EnableSecureEventInput() -> i32;
        pub(super) fn DisableSecureEventInput() -> i32;
        pub(super) fn IsSecureEventInputEnabled() -> u8;
    }
}

impl SecureInputBackend for SystemSecureInput {
    fn enable(&mut self) {
        #[cfg(target_os = "macos")]
        // SAFETY: Carbon call with no arguments; the system reference-counts enables.
        unsafe {
            carbon::EnableSecureEventInput();
        }
    }

    fn disable(&mut self) {
        #[cfg(target_os = "macos")]
        // SAFETY: Carbon call with no arguments; paired with exactly one earlier enable.
        unsafe {
            carbon::DisableSecureEventInput();
        }
    }
}

/// Secure Keyboard Entry, scoped the way Apple's guidance recommends: on only while a secret prompt
/// has focus, because it is system-wide (it blocks other apps' event taps, including some accessibility
/// tools). Calls to Enable/DisableSecureEventInput are reference-counted by the system, so every
/// enable must be paired with exactly one disable — this type guarantees that.
#[derive(Debug)]
pub struct SecureKeyboardEntry<B: SecureInputBackend = SystemSecureInput> {
    backend: B,
    is_enabled: bool,
    always_on: bool,
    last_focused: bool,
}

static SHARED: Mutex<SecureKeyboardEntry> =
    Mutex::new(SecureKeyboardEntry::with_backend(SystemSecureInput));

impl SecureKeyboardEntry {
    /// The process-wide instance (the Swift type is a `@MainActor` singleton).
    pub fn shared() -> &'static Mutex<SecureKeyboardEntry> {
        &SHARED
    }

    /// True if *any* process currently holds secure input (shown in the status bar as a lock).
    /// Always false off macOS.
    pub fn is_active_system_wide() -> bool {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: Carbon query with no arguments.
            unsafe { carbon::IsSecureEventInputEnabled() != 0 }
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
}

impl<B: SecureInputBackend> SecureKeyboardEntry<B> {
    /// A disabled instance driving `backend`.
    pub const fn with_backend(backend: B) -> Self {
        Self {
            backend,
            is_enabled: false,
            always_on: false,
            last_focused: false,
        }
    }

    /// Whether this instance currently holds a secure input reference.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    /// User preference: "Always on" mirrors Terminal.app's menu item (CIS benchmark setting).
    pub fn always_on(&self) -> bool {
        self.always_on
    }

    /// Sets the "Always on" preference and re-evaluates the state.
    pub fn set_always_on(&mut self, always_on: bool) {
        self.always_on = always_on;
        self.update(self.last_focused);
    }

    /// Call whenever focus moves into or out of a secret prompt.
    pub fn update(&mut self, secret_prompt_focused: bool) {
        self.last_focused = secret_prompt_focused;
        let want = self.always_on || secret_prompt_focused;
        if want == self.is_enabled {
            return;
        }
        if want {
            self.backend.enable();
        } else {
            self.backend.disable();
        }
        self.is_enabled = want;
    }

    /// Call from application termination / when the window resigns key.
    pub fn release_all(&mut self) {
        self.update(false);
        if self.always_on {
            self.set_always_on(false);
        }
    }

    /// The backend, for inspection.
    pub fn backend(&self) -> &B {
        &self.backend
    }
}

/// The system counts enables per process, so a dropped instance that still holds one would leave
/// Secure Event Input on (blocking other apps' event taps) until the app quits.
impl<B: SecureInputBackend> Drop for SecureKeyboardEntry<B> {
    fn drop(&mut self) {
        if self.is_enabled {
            self.backend.disable();
            self.is_enabled = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct Counting {
        enables: u32,
        disables: u32,
    }

    impl SecureInputBackend for Counting {
        fn enable(&mut self) {
            self.enables += 1;
        }
        fn disable(&mut self) {
            self.disables += 1;
        }
    }

    #[test]
    fn enables_only_while_secret_prompt_focused() {
        let mut entry = SecureKeyboardEntry::with_backend(Counting::default());
        entry.update(true);
        entry.update(true);
        assert!(entry.is_enabled());
        entry.update(false);
        assert!(!entry.is_enabled());
        assert_eq!((entry.backend().enables, entry.backend().disables), (1, 1));
    }

    #[test]
    fn always_on_keeps_it_enabled_until_release_all() {
        let mut entry = SecureKeyboardEntry::with_backend(Counting::default());
        entry.set_always_on(true);
        entry.update(true);
        entry.update(false);
        assert!(entry.is_enabled());
        entry.release_all();
        assert!(!entry.is_enabled());
        assert!(!entry.always_on());
        assert_eq!((entry.backend().enables, entry.backend().disables), (1, 1));
    }

    #[test]
    fn turning_always_on_off_restores_focus_state() {
        let mut entry = SecureKeyboardEntry::with_backend(Counting::default());
        entry.update(true);
        entry.set_always_on(true);
        entry.set_always_on(false);
        assert!(entry.is_enabled());
        assert_eq!(entry.backend().enables, 1);
    }

    /// Counts disables in a cell the test keeps, so they can be read after the entry drops.
    struct Shared(std::rc::Rc<std::cell::Cell<u32>>);

    impl SecureInputBackend for Shared {
        fn enable(&mut self) {}
        fn disable(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn dropping_an_enabled_instance_should_release_its_reference() {
        let disables = std::rc::Rc::new(std::cell::Cell::new(0));
        {
            let mut entry = SecureKeyboardEntry::with_backend(Shared(disables.clone()));
            entry.update(true);
        }
        assert_eq!(disables.get(), 1);
    }

    #[test]
    fn dropping_a_disabled_instance_should_not_release_anything() {
        let disables = std::rc::Rc::new(std::cell::Cell::new(0));
        drop(SecureKeyboardEntry::with_backend(Shared(disables.clone())));
        assert_eq!(disables.get(), 0);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn system_wide_state_is_inactive_off_macos() {
        assert!(!SecureKeyboardEntry::is_active_system_wide());
        let mut shared = SecureKeyboardEntry::shared().lock().unwrap();
        shared.update(true);
        shared.release_all();
        assert!(!shared.is_enabled());
    }
}
