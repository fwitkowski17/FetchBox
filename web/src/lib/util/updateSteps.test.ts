import { describe, expect, it } from 'vitest';
import { INSTALL_METHODS, packageSteps, repoOf, scriptStep } from './updateSteps';

describe('repoOf', () => {
  it('reads the repository from the release link', () => {
    expect(repoOf('https://github.com/me/fork/releases/tag/v1.0.0')).toBe('me/fork');
  });
  it('falls back to the project when the link is something else', () => {
    const own = 'fwitkowski17/WebDownloadManager';
    expect(repoOf(null)).toBe(own);
    expect(repoOf('https://example.com/o/r/releases/tag/v1')).toBe(own);
    expect(repoOf('https://github.com/o/r/pull/1')).toBe(own);
  });
});

describe('packageSteps', () => {
  it('downloads the package of the CPU in use and installs it', () => {
    const steps = packageSteps('o/r', '0.9.0');
    expect(steps?.deb).toEqual([
      'curl -fLO "https://github.com/o/r/releases/download/v0.9.0/fetchbox_0.9.0_$(dpkg --print-architecture).deb"',
      'sudo apt install ./fetchbox_0.9.0_$(dpkg --print-architecture).deb',
    ]);
    expect(steps?.rpm).toEqual([
      'curl -fLO "https://github.com/o/r/releases/download/v0.9.0/fetchbox-0.9.0-1.$(uname -m).rpm"',
      'sudo dnf install ./fetchbox-0.9.0-1.$(uname -m).rpm',
    ]);
  });
  it('keeps the spelling of a pre-release tag', () => {
    expect(packageSteps('o/r', '0.9.0-rc.1')?.deb[1]).toBe('sudo apt install ./fetchbox_0.9.0-rc.1_$(dpkg --print-architecture).deb');
  });
  it('refuses a version that is not safe to put in a command', () => {
    expect(packageSteps('o/r', '1.0; rm -rf /')).toBeNull();
  });
});

describe('scriptStep', () => {
  it('updates to the latest stable release', () => {
    expect(scriptStep('o/r', '0.9.0', false)).toBe(
      'curl -fsSL https://raw.githubusercontent.com/o/r/main/deploy/install.sh | sudo bash -s -- update',
    );
  });
  it('pins a pre-release', () => {
    expect(scriptStep('o/r', '0.9.0-rc.1', true)).toMatch(/-- update --version 0\.9\.0-rc\.1$/);
  });
  it('refuses an unsafe version', () => {
    expect(scriptStep('o/r', '$(evil)', true)).toBeNull();
  });
});

describe('INSTALL_METHODS', () => {
  it('lists each method once', () => {
    expect(new Set(INSTALL_METHODS).size).toBe(INSTALL_METHODS.length);
  });
});
