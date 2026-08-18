//! [`SessionWorld`]: our implementation of typst's `World` and `IdeWorld`.

use std::sync::Mutex;

use ecow::EcoString;
use typst::diag::FileResult;
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_ide::IdeWorld;

use crate::fonts::FontSlots;
use crate::packages::Packages;
use crate::ports::{ClockProvider, FileProvider, FontProvider, PackageProvider};
use crate::vfs::Vfs;

/// The environment one compile session typesets in.
pub struct SessionWorld<F, T, P, C> {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: FontSlots<T>,
    vfs: Vfs<F>,
    packages: Packages<P>,
    clock: C,
    /// The instant this compile started, memoized so `today()` cannot change
    /// mid-compile — comemo's constraint validation depends on it not moving.
    now: Mutex<Option<Option<i64>>>,
    main: FileId,
}

impl<F, T, P, C> SessionWorld<F, T, P, C>
where
    F: FileProvider,
    T: FontProvider,
    P: PackageProvider,
    C: ClockProvider,
{
    /// Build a world around the four ports.
    pub fn new(files: F, fonts: T, packages: P, clock: C, main: FileId) -> Self {
        let fonts = FontSlots::new(fonts);
        let book = fonts.book();
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            vfs: Vfs::new(files),
            packages: Packages::new(packages),
            clock,
            now: Mutex::new(None),
            main,
        }
    }

    /// The file that is currently the compile entry point.
    pub fn main_id(&self) -> FileId {
        self.main
    }

    /// Point the compiler at a different entry file.
    pub fn set_main(&mut self, main: FileId) {
        self.main = main;
    }

    /// The virtual file system, for opening and editing documents.
    pub fn vfs(&self) -> &Vfs<F> {
        &self.vfs
    }

    /// Mutable access, for `didOpen` / `didChange` / `didClose`.
    pub fn vfs_mut(&mut self) -> &mut Vfs<F> {
        &mut self.vfs
    }

    /// Package bookkeeping, for reporting what a compile still needs.
    pub fn packages(&self) -> &Packages<P> {
        &self.packages
    }

    /// The lazily-loaded font slots.
    pub fn fonts(&self) -> &FontSlots<T> {
        &self.fonts
    }

    /// Rebuild the font book after the host finishes indexing system fonts.
    pub fn set_fonts(&mut self, fonts: T) {
        self.fonts = FontSlots::new(fonts);
        self.book = LazyHash::new(self.fonts.book());
    }

    /// Prepare for a new compile: drop host reads and unfreeze the clock.
    pub fn reset(&self) {
        self.vfs.reset();
        *self.now.lock().unwrap() = None;
    }

    /// Resolve a project-relative path to a file id under the project root.
    pub fn project_file(vpath: VirtualPath) -> FileId {
        FileId::new(RootedPath::new(VirtualRoot::Project, vpath))
    }

    fn now_ms(&self) -> Option<i64> {
        let mut slot = self.now.lock().unwrap();
        *slot.get_or_insert_with(|| self.clock.now_ms())
    }
}

impl<F, T, P, C> World for SessionWorld<F, T, P, C>
where
    F: FileProvider + Send + Sync,
    T: FontProvider + Send + Sync,
    P: PackageProvider + Send + Sync,
    C: ClockProvider + Send + Sync,
{
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.packages.gate(id.get().root())?;
        self.vfs.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.packages.gate(id.get().root())?;
        self.vfs.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        let now_ms = self.now_ms()?;
        let offset_minutes = match offset {
            Some(duration) => total_minutes(&duration),
            None => self.clock.local_offset_minutes(),
        };
        datetime_at(now_ms, offset_minutes)
    }
}

impl<F, T, P, C> IdeWorld for SessionWorld<F, T, P, C>
where
    F: FileProvider + Send + Sync,
    T: FontProvider + Send + Sync,
    P: PackageProvider + Send + Sync,
    C: ClockProvider + Send + Sync,
{
    fn upcast(&self) -> &dyn World {
        self
    }

    fn packages(&self) -> &[(PackageSpec, Option<EcoString>)] {
        self.packages.provider().index()
    }

    fn files(&self) -> Vec<FileId> {
        self.vfs.known_ids()
    }
}

/// Whole minutes in a typst `Duration`.
///
/// `Duration` only exposes `decompose`, so the components are recombined.
fn total_minutes(duration: &Duration) -> i64 {
    let [weeks, days, hours, minutes, seconds] = duration.decompose();
    weeks * 7 * 24 * 60 + days * 24 * 60 + hours * 60 + minutes + seconds / 60
}

/// Convert an epoch instant plus a UTC offset into a typst `Datetime`.
fn datetime_at(now_ms: i64, offset_minutes: i64) -> Option<Datetime> {
    let offset =
        time::UtcOffset::from_whole_seconds(i32::try_from(offset_minutes * 60).ok()?)
            .ok()?;
    let instant = time::OffsetDateTime::from_unix_timestamp_nanos(
        i128::from(now_ms) * 1_000_000,
    )
    .ok()?
    .to_offset(offset);

    Datetime::from_ymd_hms(
        instant.year(),
        instant.month() as u8,
        instant.day(),
        instant.hour(),
        instant.minute(),
        instant.second(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Epoch milliseconds for a UTC wall-clock time.
    fn utc_ms(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> i64 {
        let date = time::Date::from_calendar_date(
            year,
            time::Month::try_from(month).unwrap(),
            day,
        )
        .unwrap();
        let time = time::Time::from_hms(hour, minute, 0).unwrap();
        date.with_time(time).assume_utc().unix_timestamp() * 1000
    }

    fn ymd(datetime: &Datetime) -> (Option<i32>, Option<u32>, Option<u32>) {
        (datetime.year(), datetime.month().map(u32::from), datetime.day().map(u32::from))
    }

    #[test]
    fn a_positive_offset_can_move_the_date_forward() {
        // 23:30 UTC is already the next day one hour east.
        let epoch_ms = utc_ms(2026, 8, 17, 23, 30);
        assert_eq!(ymd(&datetime_at(epoch_ms, 0).unwrap()), (Some(2026), Some(8), Some(17)));
        assert_eq!(
            ymd(&datetime_at(epoch_ms, 60).unwrap()),
            (Some(2026), Some(8), Some(18))
        );
    }

    #[test]
    fn a_negative_offset_can_move_the_date_back() {
        let epoch_ms = utc_ms(2026, 8, 17, 0, 30);
        assert_eq!(
            ymd(&datetime_at(epoch_ms, -120).unwrap()),
            (Some(2026), Some(8), Some(16))
        );
    }

    #[test]
    fn a_duration_offset_is_read_in_whole_minutes() {
        let two_hours = Duration::from(time::Duration::hours(2));
        assert_eq!(total_minutes(&two_hours), 120);
    }
}
