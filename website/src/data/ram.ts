import type { ComparisonData } from '../lib/ram';

// User-requested rounded comparison values and upper bounds.
// Original competitor readings: Crosshair X 383.7 / 382.5; CrossOver 254.2 / 221.9 MiB.
// Test hardware and methodology were not supplied. Do not invent them.
export const ramComparison = {
  status: 'reported',
  unit: 'MiB',
  basis:
    'User-requested approximations. Less-than values plot their stated upper bounds.',
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
  ],
} satisfies ComparisonData;
