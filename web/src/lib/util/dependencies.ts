// The "additional dependencies" of a plugin that is about to be installed: the programs FetchBox
// downloads for it. They are not permissions; the review shows them apart, says which of them will
// be downloaded, and asks for a yes before anything is. The server decides what happens to each one
// (`dependency_plan`, with the rule the installer follows); this only arranges it.
import type { DependencyNeed, DependencyPlan, StagedPlugin } from '../api/types';

export interface DependencyRow {
  need: DependencyNeed;
  plan: DependencyPlan;
  /** An update adds it: the installed version did not have it. */
  added: boolean;
}

export interface DependencyReview {
  rows: DependencyRow[];
  /** The ones that are downloaded when the extension is switched on. */
  downloads: DependencyRow[];
  /** Bytes of the downloads whose size is known. */
  bytes: number;
  /** How many downloads have no size to show (a GitHub release is not asked about while reviewing). */
  unknownSizes: number;
  /** The user has to agree before installing: something is going to be downloaded. */
  confirm: boolean;
  /** An update of an extension that is on downloads at once; a new one, when it is switched on. */
  when: 'now' | 'on_enable';
}

export function dependencyReview(staged: StagedPlugin): DependencyReview {
  const plans = new Map(staged.dependency_plan.map((p) => [p.id, p.plan]));
  const before = new Set((staged.replaces?.permissions.dependencies ?? []).map((d) => d.id));
  const rows = staged.permissions.dependencies.map((need) => ({
    need,
    // The server always says; if it did not, a required program is downloaded and an optional one waits.
    plan: plans.get(need.id) ?? (need.optional ? 'on_request' : 'download'),
    added: staged.replaces != null && !before.has(need.id),
  }));
  const downloads = rows.filter((r) => r.plan === 'download');
  return {
    rows,
    downloads,
    bytes: downloads.reduce((sum, r) => sum + (r.need.size ?? 0), 0),
    unknownSizes: downloads.filter((r) => r.need.size == null).length,
    confirm: downloads.length > 0,
    when: staged.replaces?.enabled ? 'now' : 'on_enable',
  };
}
