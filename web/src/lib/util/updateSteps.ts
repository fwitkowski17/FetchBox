// The commands of the update dialog. Which ones are shown depends on how FetchBox was installed
// (`VersionInfo.install_method`); the repository is read from the release link, so a fork or a
// mirror gets its own addresses.
import type { VersionInfo } from '../api/types';

export type InstallMethod = NonNullable<VersionInfo['install_method']>;

/** Every method, in the order they are listed when the installer did not say which one was used. */
export const INSTALL_METHODS: readonly InstallMethod[] = ['docker', 'package', 'lxc', 'script', 'source'];

const DEFAULT_REPO = 'fwitkowski17/WebDownloadManager';

/** `owner/repo` of a GitHub release link; the project's own repository when the link is not one. */
export function repoOf(releaseUrl: string | null | undefined): string {
  const match = /^https:\/\/github\.com\/([\w.-]+\/[\w.-]+)\/releases\//.exec(releaseUrl ?? '');
  return match?.[1] ?? DEFAULT_REPO;
}

/** A version that can go into a shell command: digits, letters, dots and dashes only. */
function safeVersion(version: string): string | null {
  return /^[0-9A-Za-z.-]+$/.test(version) ? version : null;
}

export interface PackageSteps {
  deb: string[];
  rpm: string[];
}

/** Download the package of `version` for this CPU, then install it over the old one. */
export function packageSteps(repo: string, version: string): PackageSteps | null {
  const v = safeVersion(version);
  if (!v) return null;
  const base = `https://github.com/${repo}/releases/download/v${v}`;
  const deb = `fetchbox_${v}_$(dpkg --print-architecture).deb`;
  const rpm = `fetchbox-${v}-1.$(uname -m).rpm`;
  return {
    deb: [`curl -fLO "${base}/${deb}"`, `sudo apt install ./${deb}`],
    rpm: [`curl -fLO "${base}/${rpm}"`, `sudo dnf install ./${rpm}`],
  };
}

/** `install.sh update`; a pre-release is installed only when asked for by its version. */
export function scriptStep(repo: string, version: string, prerelease: boolean): string | null {
  const v = safeVersion(version);
  if (!v) return null;
  const pin = prerelease ? ` --version ${v}` : '';
  return `curl -fsSL https://raw.githubusercontent.com/${repo}/main/deploy/install.sh | sudo bash -s -- update${pin}`;
}
