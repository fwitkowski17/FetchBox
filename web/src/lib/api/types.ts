// Friendly names for the generated schema types. Never edit schema.d.ts by hand.
import type { components } from './schema';

type Schemas = components['schemas'];

export type Download = Schemas['Download'];
export type Settings = Schemas['Settings'];
export type Category = Schemas['Category'];
export type Status = Schemas['Status'];
export type Conflict = Schemas['Conflict'];
export type ConflictKind = Schemas['ConflictKind'];
export type OnConflict = Schemas['OnConflict'];
export type AddRequest = Schemas['AddRequest'];
export type AppEvent = Schemas['Event'];
export type AuthStatus = Schemas['AuthStatus'];
export type DiskSpace = Schemas['DiskSpace'];
export type UrlInfo = Schemas['UrlInfo'];
export type VersionInfo = Schemas['VersionInfo'];
export type Hold = Schemas['Hold'];
export type ChecksumAlgo = Schemas['ChecksumAlgo'];
export type ChecksumState = Schemas['ChecksumState'];
export type ListingEntry = Schemas['ListingEntry'];
export type Credentials = Schemas['Credentials'];
export type KnownHost = Schemas['KnownHost'];
export type TrustHost = Schemas['TrustHost'];
export type HostKeyProblem = Schemas['HostKeyProblem'];
export type Stats = Schemas['Stats'];
export type StatsRange = Schemas['StatsRange'];
export type DayStat = Schemas['DayStat'];
export type CategoryStat = Schemas['CategoryStat'];

export const CATEGORIES: readonly Category[] = [
  'photo',
  'music',
  'video',
  'program',
  'archive',
  'other',
];
