/**
 * What the kitchen constructor offers: the wall sizes it accepts and the modules
 * that can go in a slot.
 */

/** A range of whole centimetres, both ends included. */
export interface CentimetreRange {
  min: number;
  max: number;
}

/**
 * Wall sizes the constructor accepts, in centimetres. Rust validates the same
 * limits again (`WALL_WIDTH_CM` / `WALL_HEIGHT_CM` in
 * `crates/wasm-kitchen/src/layout.rs`), so keep the two in step.
 */
export const WALL_LIMITS = {
  width: { min: 200, max: 500 },
  height: { min: 250, max: 300 },
} as const satisfies Record<string, CentimetreRange>;

/** The wall the user asked for. */
export interface WallDimensions {
  widthCm: number;
  heightCm: number;
}

/** Width of one module, in centimetres - how many fit is `floor(width / 80)`. */
export const MODULE_WIDTH_CM = 80;

/** The see-through box standing in every empty slot. */
export const PLACEHOLDER_URL = '/models/kitchen-placeholder-box.glb';

/** A module that can be placed in a slot. */
export interface KitchenModule {
  /** Stable id Rust caches the uploaded model under. */
  id: string;
  name: string;
  description: string;
  url: string;
  /** CSS background suggesting the finish, for the picker. */
  swatch: string;
}

export const KITCHEN_MODULES: readonly KitchenModule[] = [
  {
    id: 'polished-steel',
    name: 'Polished steel',
    description: 'Two-drawer base unit with mirror-polished steel fronts',
    url: '/models/kitchen-module-1.glb',
    swatch: 'linear-gradient(135deg, #f4f6f9 0%, #9aa3ad 38%, #eef1f4 55%, #7d8791 100%)',
  },
  {
    id: 'aluminium',
    name: 'Aluminium',
    description: 'Two-drawer base unit with satin aluminium fronts',
    url: '/models/kitchen-module-1-aluminium.glb',
    swatch: 'linear-gradient(135deg, #eceeef 0%, #c9cdd0 50%, #b5babe 100%)',
  },
];

/** How many modules fit side by side along a wall of this width. */
export function slotCountFor(widthCm: number): number {
  return Math.floor(widthCm / MODULE_WIDTH_CM);
}
