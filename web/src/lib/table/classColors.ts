import type { Series, TimingClassColor } from '$lib/types';

const wecStaticColors: Record<string, string> = {
  HYPER: '#e21e19',
  HYPERCAR: '#e21e19',
  LMH: '#e21e19',
  LMGT3: '#0b9314',
  LMP1: '#ff1053',
  LMP2: '#3f90da',
  LMGTE: '#ffa912',
  INV: '#ffffff'
};

const standardStaticColors: Record<string, string> = {
  GTP: '#ffffff',
  LMP2: '#3f90da',
  'GTD-PRO': '#d22630',
  GTD: '#00a651',
  LMH: '#dc143c',
  LMGT3: '#1e90ff',
  PRO: '#e67e22',
  'PRO-AM': '#4caf50',
  MASTERS: '#f1d302'
};

function looksLikeHexColor(value: string | undefined): boolean {
  if (!value) return false;
  return /^#[0-9a-fA-F]{6}$/.test(value.trim());
}

function resolveLiveClassColor(
  classColors: Record<string, TimingClassColor>,
  classKey: string
): string | null {
  const direct = Object.prototype.hasOwnProperty.call(classColors, classKey)
    ? classColors[classKey]
    : undefined;
  if (direct && looksLikeHexColor(direct.color)) {
    return direct.color.trim();
  }

  return null;
}

export function resolveClassTextColor(
  series: Series,
  className: string,
  classColors: Record<string, TimingClassColor>
): string | null {
  if (series === 'nls' || series === 'dhlm') {
    return null;
  }

  const key = className.trim().toUpperCase();
  const liveColor = resolveLiveClassColor(classColors, key);
  if (liveColor) {
    return liveColor;
  }

  if (series === 'wec') {
    return wecStaticColors[key] ?? null;
  }

  return standardStaticColors[key] ?? null;
}
