import { describe, it, expect } from 'vitest';
import { VPL } from '../vpl.js';

describe('VPL Builder', () => {
	describe('toString serialization', () => {
		it('should serialize fromContainer with required filename', () => {
			const vpl = VPL.fromContainer({ filename: 'world.versatiles' });
			expect(vpl.toString()).toBe('from_container filename=world.versatiles');
		});

		it('should quote filenames with special characters', () => {
			const vpl = VPL.fromContainer({ filename: 'path/to/my tiles.versatiles' });
			expect(vpl.toString()).toBe('from_container filename="path/to/my tiles.versatiles"');
		});

		it('should serialize a pipeline chain', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' })
				.rasterFormat({ format: 'webp', quality: '80' })
				.filter({ levelMin: 0, levelMax: 14 });
			expect(vpl.toString()).toBe(
				'from_container filename=test.versatiles | raster_format format=webp quality=80 | filter level_min=0 level_max=14',
			);
		});

		it('should serialize bounding box as array', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' }).filter({
				bbox: [13.0, 52.0, 14.0, 53.0],
			});
			expect(vpl.toString()).toBe('from_container filename=test.versatiles | filter bbox=[13,52,14,53]');
		});

		it('should serialize boolean values', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' }).vectorFilterLayers({
				filter: 'water,roads',
				invert: true,
			});
			expect(vpl.toString()).toBe(
				'from_container filename=test.versatiles | vector_filter_layers filter="water,roads" invert=true',
			);
		});

		it('should serialize fromDebug with optional format', () => {
			expect(VPL.fromDebug().toString()).toBe('from_debug');
			expect(VPL.fromDebug({ format: 'png' }).toString()).toBe('from_debug format=png');
		});

		it('should serialize sources with brackets', () => {
			const source1 = VPL.fromContainer({ filename: 'a.versatiles' });
			const source2 = VPL.fromContainer({ filename: 'b.versatiles' });
			const merged = VPL.fromStacked([source1, source2]);
			expect(merged.toString()).toBe(
				'from_stacked [ from_container filename=a.versatiles, from_container filename=b.versatiles ]',
			);
		});

		it('should serialize fromStackedRaster with sources and options', () => {
			const source1 = VPL.fromContainer({ filename: 'a.versatiles' });
			const source2 = VPL.fromContainer({ filename: 'b.versatiles' });
			const stacked = VPL.fromStackedRaster([source1, source2], { format: 'png' });
			expect(stacked.toString()).toBe(
				'from_stacked_raster [ from_container filename=a.versatiles, from_container filename=b.versatiles ] format=png',
			);
		});

		it('should handle nested pipeline in sources', () => {
			const source = VPL.fromContainer({ filename: 'a.versatiles' }).filter({ levelMax: 10 });
			const stacked = VPL.fromStacked([source]);
			expect(stacked.toString()).toBe('from_stacked [ from_container filename=a.versatiles | filter level_max=10 ]');
		});

		it('should escape double quotes in string values', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' }).metaUpdate({
				description: 'A "quoted" value',
			});
			expect(vpl.toString()).toBe(
				'from_container filename=test.versatiles | meta_update description="A \\"quoted\\" value"',
			);
		});
	});

	describe('immutability', () => {
		it('should not modify the original VPL when chaining', () => {
			const base = VPL.fromContainer({ filename: 'test.versatiles' });
			const filtered = base.filter({ levelMax: 10 });
			expect(base.toString()).toBe('from_container filename=test.versatiles');
			expect(filtered.toString()).toBe('from_container filename=test.versatiles | filter level_max=10');
		});
	});

	describe('type safety', () => {
		it('should accept all filter options', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' }).filter({
				bbox: [0, 0, 180, 90],
				levelMin: 0,
				levelMax: 14,
			});
			expect(vpl.toString()).toContain('filter');
		});

		it('should accept fromColor with no options', () => {
			const vpl = VPL.fromColor();
			expect(vpl.toString()).toBe('from_color');
		});

		it('should accept fromColor with options', () => {
			const vpl = VPL.fromColor({ color: 'FF5733', size: 256 });
			expect(vpl.toString()).toBe('from_color color=FF5733 size=256');
		});

		it('should accept rasterLevels with brightness', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' }).rasterLevels({
				brightness: 10,
				contrast: 1.5,
			});
			expect(vpl.toString()).toContain('brightness=10');
			expect(vpl.toString()).toContain('contrast=1.5');
		});
	});
});
