import type { Series, TimingClassColor } from '../types';

const WEC_STATIC_COLORS: Record<string, string> = {
  HYPER: '#e21e19',
  HYPERCAR: '#e21e19',
  INV: '#ffffff',
  LMGT3: '#0b9314',
  LMGTE: '#ffa912',
  LMH: '#e21e19',
  LMP1: '#ff1053',
  LMP2: '#3f90da',
};

const STANDARD_STATIC_COLORS: Record<string, string> = {
  GTD: '#00a651',
  'GTD-PRO': '#d22630',
  GTP: '#ffffff',
  LMGT3: '#1e90ff',
  LMH: '#dc143c',
  LMP2: '#3f90da',
  MASTERS: '#f1d302',
  PRO: '#e67e22',
  'PRO-AM': '#4caf50',
};

const HEX_COLOR_REGEX = /^#[0-9a-fA-F]{6}$/u;

const looksLikeHexColor = function looksLikeHexColor(
  value: string | undefined,
): boolean {
  if (value === undefined || value === '') {
    return false;
  }
  return HEX_COLOR_REGEX.test(value.trim());
};

const resolveLiveClassColor = function resolveLiveClassColor(
  classColors: Record<string, TimingClassColor>,
  classKey: string,
): string | null {
  const direct = Object.hasOwn(classColors, classKey)
    ? classColors[classKey]
    : undefined;
  if (direct !== undefined && looksLikeHexColor(direct.color)) {
    return direct.color.trim();
  }

  return null;
};

export const resolveClassTextColor = function resolveClassTextColor(
  series: Series,
  className: string,
  classColors: Record<string, TimingClassColor>,
): string | null {
  if (series === 'dhlm' || series === 'nls') {
    return null;
  }

  const key = className.trim().toUpperCase();
  const liveColor = resolveLiveClassColor(classColors, key);
  if (liveColor !== null && liveColor !== '') {
    return liveColor;
  }

  if (series === 'wec') {
    return WEC_STATIC_COLORS[key] ?? null;
  }

  return STANDARD_STATIC_COLORS[key] ?? null;
};
