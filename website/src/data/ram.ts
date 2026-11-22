import type { ComparisonData } from '../lib/ram';

// Earlier readings: Crosshair X 383.7 / 382.5; CrossOver 254.2 / 221.9 MiB.
// New sample medians: Crosshair V2 121.20; OpenCrosshair 161.44 / 2.33 MiB.
export const ramComparison = {
  status: 'reported',
  unit: 'MiB',
  basis:
    'Crosshair V2 and OpenCrosshair use the median of three Windows private working set samples per tested state. The earlier readings use an unrecorded method. Less-than values are plotted at their upper bounds.',
  apps: [
    {
      id: 'crosshair-rs',
      version: 'alpha.6',
      menuOpen: 50,
      menuClosed: 5,
      bounds: { menuOpen: 'lessThan', menuClosed: 'lessThan' },
    },
    {
      id: 'crosshair-x',
      version: '15.5.6',
      menuOpen: 400,
      menuClosed: 400,
    },
    { id: 'crossover', version: '3.4.9', menuOpen: 250, menuClosed: 220 },
    { id: 'crosshair-v2', version: '1.1.28.0', menuOpen: null, menuClosed: 120 },
    {
      id: 'opencrosshair',
      version: '0.5.10',
      menuOpen: 160,
      menuClosed: 5,
      bounds: { menuClosed: 'lessThan' },
    },
  ],
} satisfies ComparisonData;
