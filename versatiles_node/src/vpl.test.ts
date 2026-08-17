import { describe, it, expect } from 'vitest';
import { VPL } from '../vpl.js';
import { parseVpl } from '../index.js';
import { parseCst, parseCstResult, stringifyCst } from '../vpl.js';

/**
 * Asserts the serialised pipeline, and that it is actually valid VPL.
 *
 * Comparing strings alone is what let `from_stacked_raster [ … ] format=png` sit in this file:
 * it looked plausible and the grammar rejects it, because sources have to follow the parameters.
 */
function expectVpl(vpl: VPL, expected: string): void {
	const text = vpl.toString();
	expect(text).toBe(expected);
	const result = JSON.parse(parseVpl(text)) as { ok: boolean; error?: { trace: string } };
	expect(result.ok, `should be valid VPL but was not:\n${result.error?.trace}`).toBe(true);
}

describe('VPL Builder', () => {
	describe('toString serialization', () => {
		it('should serialize fromContainer with required filename', () => {
			const vpl = VPL.fromContainer({ filename: 'world.versatiles' });
			expect(vpl.toString()).toBe('from_container filename=world.versatiles');
		});

		it('should quote filenames with special characters', () => {
			const vpl = VPL.fromContainer({ filename: 'path/to/my tiles.versatiles' });
			// single quotes need no escapes, so the grammar's serialiser prefers them
			expectVpl(vpl, "from_container filename='path/to/my tiles.versatiles'");
		});

		it('should serialize a pipeline chain', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' })
				.rasterFormat({ format: 'webp', quality: '80' })
				.filter({ levelMin: 0, levelMax: 14 });
			// parameters come back alphabetised: the semantic tree holds them in a sorted map
			expectVpl(
				vpl,
				'from_container filename=test.versatiles | raster_format format=webp quality=80 | filter level_max=14 level_min=0',
			);
		});

		it('should serialize bounding box as array', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' }).filter({
				bbox: [13.0, 52.0, 14.0, 53.0],
			});
			expectVpl(vpl, 'from_container filename=test.versatiles | filter bbox=[13, 52, 14, 53]');
		});

		it('should serialize boolean values', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' }).vectorFilterLayers({
				filter: 'water,roads',
				invert: true,
			});
			expectVpl(vpl, "from_container filename=test.versatiles | vector_filter_layers filter='water,roads' invert=true");
		});

		it('should serialize fromDebug with optional format', () => {
			expectVpl(VPL.fromDebug(), 'from_debug');
			expectVpl(VPL.fromDebug({ format: 'png' }), 'from_debug format=png');
		});

		it('should serialize sources with brackets', () => {
			const source1 = VPL.fromContainer({ filename: 'a.versatiles' });
			const source2 = VPL.fromContainer({ filename: 'b.versatiles' });
			const merged = VPL.fromStacked([source1, source2]);
			expectVpl(merged, 'from_stacked [ from_container filename=a.versatiles, from_container filename=b.versatiles ]');
		});

		it('should serialize fromStackedRaster with sources and options', () => {
			const source1 = VPL.fromContainer({ filename: 'a.versatiles' });
			const source2 = VPL.fromContainer({ filename: 'b.versatiles' });
			const stacked = VPL.fromStackedRaster([source1, source2], { format: 'png' });
			// the parameters come *before* the sources; the other order does not parse
			expectVpl(
				stacked,
				'from_stacked_raster format=png [ from_container filename=a.versatiles, from_container filename=b.versatiles ]',
			);
		});

		it('should handle nested pipeline in sources', () => {
			const source = VPL.fromContainer({ filename: 'a.versatiles' }).filter({ levelMax: 10 });
			const stacked = VPL.fromStacked([source]);
			expectVpl(stacked, 'from_stacked [ from_container filename=a.versatiles | filter level_max=10 ]');
		});

		it('should escape double quotes in string values', () => {
			const vpl = VPL.fromContainer({ filename: 'test.versatiles' }).metaUpdate({
				description: 'A "quoted" value',
			});
			// no escaping needed once single quotes are chosen
			expectVpl(vpl, 'from_container filename=test.versatiles | meta_update description=\'A "quoted" value\'');
		});
	});

	describe('immutability', () => {
		it('should not modify the original VPL when chaining', () => {
			const base = VPL.fromContainer({ filename: 'test.versatiles' });
			const filtered = base.filter({ levelMax: 10 });
			expectVpl(base, 'from_container filename=test.versatiles');
			expectVpl(filtered, 'from_container filename=test.versatiles | filter level_max=10');
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
			expectVpl(vpl, 'from_color');
		});

		it('should accept fromColor with options', () => {
			const vpl = VPL.fromColor({ color: 'FF5733', size: 256 });
			expectVpl(vpl, 'from_color color=FF5733 size=256');
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

	describe('parsing', () => {
		it('round-trips text through parse and toString', () => {
			for (const source of [
				"from_container filename='a b'",
				'from_container filename=test.versatiles | filter level_max=14',
				'from_stacked [ from_color color=FF0000, from_color color=00FF00 ]',
				"node key=''",
			]) {
				expect(VPL.parse(source).toString()).toBe(source);
			}
		});

		it('reports a syntax error as data, with byte offsets', () => {
			const error = VPL.parseError('from_container filename=a | vector_filter zoom');
			expect(error).toBeDefined();
			expect(error?.span).toEqual({ start: 46, end: 46 });
			expect(error?.message).toBe("expected '=', got end of input");
			expect(error?.context[0]).toEqual({ label: 'parsing property', offset: 42 });
			expect(error?.trace).toContain('at line 1');
		});

		it('offsets are bytes, so multi-byte characters do not shift them', () => {
			const error = VPL.parseError('node a="Grüße" b=!');
			expect(error?.span).toEqual({ start: 19, end: 20 });
		});

		it('returns undefined for valid input', () => {
			expect(VPL.parseError('from_debug')).toBeUndefined();
		});

		it('throws from parse, carrying the structured error', () => {
			expect(() => VPL.parse('node ][')).toThrow(SyntaxError);
			try {
				VPL.parse('node ][');
			} catch (error) {
				expect((error as SyntaxError & { vpl: { trace: string } }).vpl.trace).toContain('at line 1');
			}
		});
	});
});

describe('CST (lossless syntax tree)', () => {
	const source = "# which file\nfrom_container  filename = 'berlin.versatiles'  # note\n| filter level_min=5\n";

	it('round-trips a file byte for byte', () => {
		expect(stringifyCst(parseCst(source))).toBe(source);
	});

	it('changes only what was edited', () => {
		const cst = parseCst(source);
		const value = cst.pipeline.nodes[0].value.properties![0].value;
		if (value.kind !== 'single') throw new Error('expected a single value');
		value.token.text = 'hamburg.versatiles';
		value.quote = 'bare';

		expect(stringifyCst(cst)).toBe(
			'# which file\nfrom_container  filename = hamburg.versatiles  # note\n| filter level_min=5\n',
		);
	});

	it('keeps parameters in source order, unlike the semantic tree', () => {
		const cst = parseCst('node zebra=1 alpha=2');
		expect(cst.pipeline.nodes[0].value.properties!.map((p) => p.key.text)).toEqual(['zebra', 'alpha']);
		expect(VPL.parse('node zebra=1 alpha=2').toString()).toBe('node alpha=2 zebra=1');
	});

	it('gives every token a byte span into the source', () => {
		const cst = parseCst(source);
		const name = cst.pipeline.nodes[0].value.name;
		expect(source.slice(name.span!.start, name.span!.end)).toBe('from_container');
	});

	it('accepts a tree built by hand, without the formatting fields', () => {
		expect(stringifyCst({ pipeline: { nodes: [{ value: { name: { text: 'from_debug' } } }] } })).toBe('from_debug');
	});

	it('reports a syntax error as data, or throws', () => {
		const result = parseCstResult('node ][');
		expect(result.ok).toBe(false);
		expect(() => parseCst('node ][')).toThrow(SyntaxError);
	});
});
