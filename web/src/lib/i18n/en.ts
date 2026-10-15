// English: the source of truth. Its keys define `MessageKey`; every other language must provide
// all of them (checked by the compiler and by locales.test.ts).
//
// Conventions
//   - `{name}` is a placeholder. A plural message is an object and takes its form from `{n}`.
//   - Keep the brand name "dlmanager" out of the dictionaries; it is not translated.
//   - Avoid building sentences from pieces: translators need whole sentences.
import type { Message } from './types';

export const en = {
  // ---- common
  'common.cancel': 'Cancel',
  'common.tryAgain': 'Try again',
  'common.loading': 'Loading…',
  'common.dismiss': 'Dismiss',
  'common.optional': '(optional)',

  // ---- navigation and shell
  'nav.main': 'Main',
  'nav.downloads': 'Downloads',
  'nav.add': 'Add',
  'nav.settings': 'Settings',
  'nav.open': 'Open menu',
  'nav.close': 'Close menu',
  'nav.filters': 'Filters',
  'filters.status': 'Status',
  'filters.categories': 'Categories',
  'filters.active': 'Active filters',
  'filters.remove': 'Remove filter: {name}',
  'app.offline': 'Cannot reach the server.',
  'app.totalSpeed': 'Total download speed',
  'connection.live': 'Live',
  'connection.connecting': 'Reconnecting…',
  'connection.offline': 'Offline',
  'connection.title': 'Connection to the server: {state}',
  'theme.system': 'System',
  'theme.light': 'Light',
  'theme.dark': 'Dark',
  'theme.black': 'Black',
  'theme.button': 'Theme: {theme}. Activate to change.',

  // ---- sign in
  'login.prompt': 'Enter the password to continue.',
  'login.password': 'Password',
  'login.submit': 'Sign in',
  'login.submitting': 'Signing in…',
  'login.wrong': 'Wrong password.',
  'login.locked': 'Too many attempts. Try again in {n} s.',
  'login.failed': 'Login failed.',

  // ---- download list
  'downloads.title': 'Downloads',
  'downloads.summary.active': '{n} active',
  'downloads.summary.queued': '{n} queued',
  'downloads.summary.idle': 'Nothing running',
  'downloads.clearFinished': 'Clear finished ({n})',
  'downloads.search.label': 'Search downloads',
  'downloads.search.placeholder': 'Search by name or address',
  'downloads.sort.label': 'Sort by',
  'downloads.sort.direction': 'Sort direction: {direction}. Activate to reverse.',
  'downloads.filter.status': 'Filter by status',
  'downloads.filter.category': 'Filter by category',
  'downloads.filter.allCategories': 'All categories',
  'downloads.loadFailed': 'Could not load the list: {error}',
  'downloads.empty': 'No downloads yet.',
  'downloads.empty.add': 'Add a download',
  'downloads.noMatch': 'No downloads match these filters.',
  'downloads.clearFilters': 'Clear filters',
  'downloads.cleared': {
    one: 'Removed {n} finished entry from the list',
    other: 'Removed {n} finished entries from the list',
  },

  'sort.added': 'Date added',
  'sort.name': 'Name',
  'sort.size': 'Size',
  'sort.progress': 'Progress',
  'sort.speed': 'Speed',
  'sort.status': 'Status',
  'sort.asc': 'Ascending',
  'sort.desc': 'Descending',

  'group.all': 'All',
  'group.active': 'Active',
  'group.paused': 'Paused',
  'group.completed': 'Completed',
  'group.stopped': 'Failed',

  'category.photo': 'Photos',
  'category.music': 'Music',
  'category.video': 'Video',
  'category.program': 'Programs',
  'category.archive': 'Archives',
  'category.other': 'Other',

  'status.queued': 'Queued',
  'status.downloading': 'Downloading',
  'status.paused': 'Paused',
  'status.completed': 'Completed',
  'status.failed': 'Failed',
  'status.cancelled': 'Cancelled',

  // ---- download card
  'card.progressOf': 'Progress of {name}',
  'card.waiting': 'Waiting for a free slot',
  'card.cancelled': 'Cancelled',
  'card.ofTotal': '{done} of {total}',
  'card.eta': '{time} left',
  'card.noResume': 'This server does not allow resuming, so pausing restarts the download.',
  'card.pause': 'Pause',
  'card.resume': 'Resume',
  'card.retry': 'Retry',
  'card.categoryOf': 'Category of {name}',
  'card.cancel.label': 'Cancel {name}',
  'card.cancel.title': 'Cancel',
  'card.remove.label': 'Remove {name} from the list',
  'card.remove.title': 'Remove from list',
  'card.remove.titleKeepsFile': 'Remove from list (the file stays on disk)',
  'confirm.cancel.title': 'Cancel this download?',
  'confirm.cancel.message': '“{name}” will stop and the partly downloaded file will be deleted.',
  'confirm.cancel.yes': 'Cancel download',
  'confirm.keep': 'Keep it',

  // ---- duration (seconds; minutes + seconds; hours + minutes; days + hours)
  'duration.s': '{s} s',
  'duration.ms': '{m} min {s} s',
  'duration.hm': '{h} h {m} min',
  'duration.dh': '{d} d {h} h',

  // ---- add download
  'add.title': 'Add download',
  'add.address': 'Address',
  'add.address.hint': 'Direct link to a file.',
  'add.address.invalid': 'Enter a full http:// or https:// address.',
  'add.filename': 'File name',
  'add.filename.placeholder': 'Taken from the server',
  'add.category': 'Category',
  'add.category.auto': 'Detect automatically',
  'add.submit': 'Start download',
  'add.submitting': 'Adding…',
  'add.failed': 'Could not add the download.',
  'add.added': 'Added “{name}”',

  // ---- name conflict dialog
  'conflict.title.duplicate': 'Already in your list',
  'conflict.title.name': 'File name already taken',
  'conflict.duplicate': 'This address is already a download: {name}.',
  'conflict.reserved':
    'Another download is still writing a file called {name}, so this name cannot be replaced.',
  'conflict.exists': 'A file named {name} is already in the download folder.',
  'conflict.existsDetails': 'A file named {name} is already in the download folder ({details}).',
  'conflict.hint.keepBoth': 'Keep both and the new file is saved as {name}.',
  'conflict.hint.replace': 'Or replace the existing file once the new download finishes.',
  'conflict.replace': 'Replace existing',
  'conflict.keepBoth': 'Keep both',

  // ---- settings
  'settings.title': 'Settings',
  'settings.loadFailed': 'Could not load the settings: {error}',
  'settings.downloads': 'Downloads',
  'settings.connections.label': 'Connections per download',
  'settings.connections.hint':
    'More connections can be faster, but only for servers that allow it. Applies to new downloads.',
  'settings.concurrent.label': 'Simultaneous downloads',
  'settings.concurrent.hint': 'Extra downloads wait in the queue. Changes take effect immediately.',
  'settings.speed': 'Speed limits',
  'settings.speed.each': 'Limit each download',
  'settings.speed.eachHint': 'Applies to every download separately.',
  'settings.speed.total': 'Limit total speed',
  'settings.speed.totalHint': 'Shared by all downloads together.',
  'settings.speed.max': '{label}: maximum speed',
  'settings.speed.unit': 'Unit',
  'settings.speed.unlimited': 'Unlimited',
  'settings.files': 'Files',
  'settings.subdirs': 'Sort into category folders',
  'settings.subdirs.hint':
    'New downloads go into folders like video/ or music/. Existing files are not moved.',
  'settings.unsaved': 'Unsaved changes',
  'settings.discard': 'Discard',
  'settings.save': 'Save changes',
  'settings.saving': 'Saving…',
  'settings.saved': 'Settings saved. They apply right away.',
  'settings.saveFailed': 'Could not save the settings.',
  'settings.appearance': 'Appearance',
  'settings.theme': 'Theme',
  'settings.theme.hint':
    '“System” follows the light or dark setting of your device. “Black” is dark with a pure black background, easier on OLED screens.',
  'settings.language': 'Language',
  'settings.language.auto': 'Automatic (browser language)',
  'settings.account': 'Account',
  'settings.signOut': 'Sign out',
  'settings.error.connections': 'Use a whole number from 1 to {max}.',
  'settings.error.concurrent': 'Use a whole number from 1 to {max}.',
  'settings.error.speed': 'Enter a speed above 0, or turn the limit off.',

  // ---- errors shown to the user
  'error.network': 'Cannot reach the server.',
  'error.http': 'The server answered with an error ({status}).',
  'error.unexpected': 'Something went wrong.',
} satisfies Record<string, Message>;

export type MessageKey = keyof typeof en;
export type Messages = Record<MessageKey, Message>;
