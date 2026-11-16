//! The variables the fused code reads and writes: a view of the globals'
//! table and the running DEF's variables, by [`Var::table`] and
//! [`Var::index`].
//!
//! A default build checks each read and write against its table's length,
//! as slice indexing does. The `unchecked-vars` feature swaps in a view that
//! does not, for about a tenth of the interpreter's instructions on
//! arithmetic on an in-order Arm core: the check was about four aarch64
//! instructions of the twelve a read takes, the table's length loaded,
//! compared and branched on. Both views have the same methods, so
//! `run_fused` is the same code in both builds, and this file is the only
//! one where they differ. See docs/security.md before turning it on.

#[cfg(not(feature = "unchecked-vars"))]
pub(crate) use checked::Vars;
#[cfg(feature = "unchecked-vars")]
pub(crate) use unchecked::Vars;

/// Each read and write checked, as slice indexing checks it: an entry that
/// names a place out of range panics, and the runtime stops the program
/// with `Internal Error`.
#[cfg(not(feature = "unchecked-vars"))]
mod checked {
    use crate::fuse::{Fast, Var};
    use crate::value::Value;

    pub(crate) struct Vars<'a> {
        tables: [&'a mut [Value]; 2],
    }

    impl<'a> Vars<'a> {
        /// Whether `globals` can be read by fused entries: always, as each
        /// read is checked.
        #[inline(always)]
        pub(crate) fn fits(_globals: &[Value], _global_places: usize) -> bool {
            true
        }

        /// The view of `globals` and of `locals`, the running DEF's
        /// variables.
        #[inline(always)]
        pub(crate) fn new(
            globals: &'a mut [Value],
            locals: &'a mut [Value],
            _global_places: usize,
        ) -> Vars<'a> {
            Vars {
                tables: [globals, locals],
            }
        }

        /// Whether the running DEF has the variables `fast` names: left to
        /// the check on each read.
        #[inline(always)]
        pub(crate) fn covers(&self, _fast: &Fast) -> bool {
            true
        }

        /// The value in `var`'s place.
        #[inline(always)]
        pub(crate) fn get(&self, var: Var) -> &Value {
            &self.tables[var.table()][var.index()]
        }

        /// [`Vars::get`] to write.
        #[inline(always)]
        pub(crate) fn at(&mut self, var: Var) -> &mut Value {
            &mut self.tables[var.table()][var.index()]
        }

        /// The `n` places from `var` on: a builtin's arguments.
        #[inline(always)]
        pub(crate) fn run(&self, var: Var, n: usize) -> Option<&[Value]> {
            let index = var.index();
            self.tables[var.table()].get(index..index.checked_add(n)?)
        }
    }
}

/// Reads and writes without a bounds check on each.
///
/// The invariant: every [`Var`] the running entry names is within its
/// table. It holds by three checks, each made once rather than on each
/// read:
///
/// - [`crate::fuse::check`] runs on every program the VM runs, when the VM
///   takes it and before it is shared. It makes sure that each global place
///   an entry or its chain names is below [`crate::fuse::global_places`],
///   and works out [`Fast::need`], what the entry needs of the running
///   DEF's variables. If a place is out of range, the program runs without
///   its fused code. A program the VM holds is behind an `Rc` and never
///   changes again.
/// - `run_fused` asks [`Vars::fits`] whether the globals' table is at least
///   `global_places` long before it starts, and [`Vars::new`] asserts it.
///   The table does not change length while it runs.
/// - `run_fused` asks [`Vars::covers`] before each entry, and leaves an
///   entry whose `need` is more than the running DEF's variables to `step`.
///
/// What would break it: an entry whose places `fuse::check` does not see
/// (a new [`crate::fuse::FastOp`] or [`crate::fuse::Micro`] kind left out of
/// its match, which does not compile without an arm), a program changed
/// after the check, or a view kept across a change to the variables' lists.
/// The view holds the borrows of both lists, so the compiler refuses the
/// last. Every read and write also has a `debug_assert`, so the tests,
/// which run in debug builds, catch a broken invariant as a failure rather
/// than as memory gone wrong.
///
/// A bug here is memory unsafety reachable from an untrusted program.
#[cfg(feature = "unchecked-vars")]
mod unchecked {
    use std::marker::PhantomData;

    use crate::fuse::{Fast, Var};
    use crate::value::Value;

    pub(crate) struct Vars<'a> {
        tables: [*mut Value; 2],
        lens: [usize; 2],
        _borrows: PhantomData<&'a mut [Value]>,
    }

    impl<'a> Vars<'a> {
        /// Whether `globals` holds every global place fused entries can
        /// name, which [`Vars::new`] needs.
        #[inline(always)]
        pub(crate) fn fits(globals: &[Value], global_places: usize) -> bool {
            globals.len() >= global_places
        }

        /// The view of `globals`, which must hold at least `global_places`
        /// values, and of `locals`, the running DEF's variables.
        #[inline(always)]
        pub(crate) fn new(
            globals: &'a mut [Value],
            locals: &'a mut [Value],
            global_places: usize,
        ) -> Vars<'a> {
            assert!(globals.len() >= global_places, "globals checked before");
            Vars {
                tables: [globals.as_mut_ptr(), locals.as_mut_ptr()],
                lens: [globals.len(), locals.len()],
                _borrows: PhantomData,
            }
        }

        /// Whether the running DEF has the variables `fast` names.
        #[inline(always)]
        pub(crate) fn covers(&self, fast: &Fast) -> bool {
            usize::from(fast.need) <= self.lens[1]
        }

        /// The value in `var`'s place. Only for the places of an entry that
        /// [`Vars::covers`] passed.
        #[inline(always)]
        pub(crate) fn get(&self, var: Var) -> &Value {
            let (table, index) = (var.table(), var.index());
            debug_assert!(index < self.lens[table], "{var:?} beyond {:?}", self.lens);
            // SAFETY: `index` is within its table, by the invariant above:
            // `fuse::check` bounds every global place by `global_places`,
            // which `new` asserted of the globals' table, and every local
            // place by the entry's `need`, which `covers` compared with the
            // locals'. The pointer comes from a slice this view borrows for
            // `'a`.
            unsafe { &*self.tables[table].add(index) }
        }

        /// [`Vars::get`] to write.
        #[inline(always)]
        pub(crate) fn at(&mut self, var: Var) -> &mut Value {
            let (table, index) = (var.table(), var.index());
            debug_assert!(index < self.lens[table], "{var:?} beyond {:?}", self.lens);
            // SAFETY: as in `get`; `&mut self` keeps this the only reference
            // into the tables while it lives.
            unsafe { &mut *self.tables[table].add(index) }
        }

        /// The `n` places from `var` on, checked: a builtin's arguments.
        #[inline(always)]
        pub(crate) fn run(&self, var: Var, n: usize) -> Option<&[Value]> {
            let (table, index) = (var.table(), var.index());
            let end = index.checked_add(n)?;
            if end > self.lens[table] {
                return None;
            }
            // SAFETY: `index..end` is within the table, checked just above,
            // and the pointer comes from a slice this view borrows for `'a`.
            Some(unsafe { std::slice::from_raw_parts(self.tables[table].add(index), n) })
        }
    }
}
