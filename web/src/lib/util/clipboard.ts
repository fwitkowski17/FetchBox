// Copying text. `navigator.clipboard` only exists in secure contexts (HTTPS or localhost), and a
// homelab manager is often opened over plain HTTP on the LAN, so there is a fallback.

/** Copy `text`; resolves to whether it worked. */
export async function copyText(text: string): Promise<boolean> {
  if (typeof navigator !== 'undefined' && navigator.clipboard && window.isSecureContext) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // Denied or unavailable: try the old way.
    }
  }
  return copyWithSelection(text);
}

function copyWithSelection(text: string): boolean {
  const area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  // Off screen, but real: a hidden element cannot be selected.
  area.style.cssText = 'position:fixed;top:0;left:-9999px;opacity:0;';
  const active = document.activeElement as HTMLElement | null;
  document.body.appendChild(area);
  area.select();
  area.setSelectionRange(0, text.length);
  let ok = false;
  try {
    ok = document.execCommand('copy');
  } catch {
    ok = false;
  }
  area.remove();
  active?.focus?.(); // selecting stole the focus
  return ok;
}
