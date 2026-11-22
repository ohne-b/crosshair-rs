export const applications = [
  {
    id: 'crosshair-rs',
    name: 'crosshair-rs',
    url: 'https://github.com/ohne-b/crosshair-rs',
  },
  { id: 'crosshair-v2', name: 'Crosshair V2', url: 'https://centerpointgaming.com/crosshairv2.html' },
  { id: 'opencrosshair', name: 'OpenCrosshair', url: 'https://github.com/Tu2525/OpenCrosshair' },
  {
    id: 'crossover',
    name: 'CrossOver',
    url: 'https://github.com/lacymorrow/crossover',
  },
  { id: 'crosshair-x', name: 'Crosshair X', url: 'https://crosshairx.gg/' },
] as const;

export const states = [
  { id: 'menuOpen', name: 'Menu open' },
  { id: 'menuClosed', name: 'Menu closed' },
] as const;

type AppId = (typeof applications)[number]['id'];
type StateId = (typeof states)[number]['id'];
interface Readings {
  id: AppId;
  menuOpen: number | null;
  menuClosed: number;
  bounds?: Partial<Record<StateId, 'lessThan'>>;
}
export type ComparisonData =
  | {
      status: 'reported';
      unit: 'MiB';
      basis: string;
      apps: (Readings & { version: string })[];
    }
  | { status: 'illustrative'; unit: 'MB'; basis: string; apps: Readings[] }
  | {
      status: 'measured';
      unit: 'MiB';
      metric: 'Private working set';
      testedOn: string;
      system: string;
      method: string;
      apps: (Readings & { version: string })[];
    };

/** Validate at build time so missing or invalid data can never silently become a published bar. */
export function createComparison(data: ComparisonData) {
  const estimated = data.status === 'illustrative';
  if (data.apps.length !== applications.length)
    throw new Error(`Expected exactly ${applications.length} apps.`);
  if (estimated) {
    if (data.unit !== 'MB' || !data.basis.trim())
      throw new Error('Estimates need MB units and provenance.');
  } else if (data.status === 'reported') {
    if (
      data.unit !== 'MiB' ||
      !data.basis.trim() ||
      data.apps.some((app) => !app.version.trim())
    )
      throw new Error(
        'Reported readings need MiB units, provenance and versions.',
      );
  } else {
    if (
      data.unit !== 'MiB' ||
      data.metric !== 'Private working set' ||
      !data.testedOn.trim() ||
      !data.system.trim() ||
      !data.method.trim()
    ) {
      throw new Error('Measurements need units, date, system and methodology.');
    }
    if (
      data.apps.some(
        (app) => !app.version.trim() || Object.keys(app.bounds ?? {}).length,
      )
    ) {
      throw new Error('Measurements need versions and exact readings.');
    }
  }
  const readings = applications.map((app) => {
    const matches = data.apps.filter((row) => row.id === app.id);
    if (matches.length !== 1)
      throw new Error(`Expected one entry for ${app.id}.`);
    const row = matches[0]!;
    if (
      !Number.isFinite(row.menuClosed) ||
      row.menuClosed <= 0 ||
      (row.menuOpen !== null && (!Number.isFinite(row.menuOpen) || row.menuOpen <= 0))
    ) {
      throw new Error(`Invalid RAM reading for ${app.id}.`);
    }
    return row;
  });
  // A less-than bar plots its stated upper bound on the shared scale.
  const maximum = Math.max(
    ...readings.flatMap((row) => [row.menuOpen ?? 0, row.menuClosed]),
  );
  const rounded = data.status === 'reported';
  return {
    heading: estimated ? 'Estimated RAM usage' : 'RAM usage',
    bars: applications.map((app, index) => {
      const row = readings[index]!;
      function reading(state: StateId, value: number) {
        const prefix =
          estimated || rounded
            ? row.bounds?.[state] === 'lessThan'
              ? '<'
              : '~'
            : '';
        return {
          percent: (100 * value) / maximum,
          label: `${prefix}${rounded ? Math.round(value) : estimated ? value : value.toFixed(1)} ${data.unit}`,
        };
      }
      return {
        ...app,
        ours: app.id === 'crosshair-rs',
        menuOpen: row.menuOpen === null ? null : reading('menuOpen', row.menuOpen),
        menuClosed: reading('menuClosed', row.menuClosed),
      };
    }),
  };
}
