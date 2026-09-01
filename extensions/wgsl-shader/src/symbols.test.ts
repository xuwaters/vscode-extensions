import { describe, expect, it } from 'vitest';
import * as path from 'path';
import * as fs from 'fs';
import { findGlslSymbols, findWgslSymbols, type ShaderSymbol } from './symbols';

const EXAMPLES = path.join(__dirname, '..', 'examples');

const names = (symbols: ShaderSymbol[], kind: ShaderSymbol['kind']) =>
  symbols.filter((s) => s.kind === kind).map((s) => s.name);

describe('WGSL symbols', () => {
  it('finds functions and structs', () => {
    const symbols = findWgslSymbols(
      ['struct VertexOutput {', '  @location(0) color: vec3f,', '};', '', 'fn vs_main() {', '}'].join(
        '\n',
      ),
    );
    expect(names(symbols, 'struct')).toEqual(['VertexOutput']);
    expect(names(symbols, 'function')).toEqual(['vs_main']);
    expect(symbols.find((s) => s.name === 'vs_main')?.line).toBe(4);
  });
});

describe('GLSL symbols', () => {
  it('finds function definitions but not calls or prototypes', () => {
    const symbols = findGlslSymbols(
      [
        'float lambert(vec3 n, vec3 l);', // prototype
        'float lambert(vec3 n, vec3 l) {',
        '    return max(dot(n, l), 0.0);',
        '}',
        'void main() {',
        '    float x = lambert(a, b);',
        '}',
      ].join('\n'),
    );
    expect(names(symbols, 'function')).toEqual(['lambert', 'main']);
  });

  it('does not mistake control flow for a declaration', () => {
    const symbols = findGlslSymbols(
      ['void main() {', '    if (x > 0.0) {', '    } else if (y > 0.0) {', '    }', '}'].join('\n'),
    );
    expect(names(symbols, 'function')).toEqual(['main']);
  });

  it('finds structs and named interface blocks', () => {
    const symbols = findGlslSymbols(
      [
        'struct Light {',
        '    vec3 color;',
        '};',
        'layout(std140, binding = 0) uniform Camera {',
        '    mat4 view;',
        '} camera;',
      ].join('\n'),
    );
    expect(names(symbols, 'struct')).toEqual(['Light', 'Camera']);
  });

  it('finds qualified globals but not locals', () => {
    const symbols = findGlslSymbols(
      [
        'layout(location = 0) in vec3 in_position;',
        'const vec3 UP = vec3(0.0, 1.0, 0.0);',
        'void main() {',
        '    vec3 local = in_position;',
        '}',
      ].join('\n'),
    );
    expect(names(symbols, 'variable')).toEqual(['in_position', 'UP']);
  });

  it('ignores declarations that only appear in a comment', () => {
    const symbols = findGlslSymbols('// void ghost() {\nvoid main() {\n}\n');
    expect(names(symbols, 'function')).toEqual(['main']);
  });

  it('outlines the shipped examples', () => {
    const vert = findGlslSymbols(fs.readFileSync(path.join(EXAMPLES, 'test.vert'), 'utf8'));
    expect(names(vert, 'function')).toEqual(['main']);
    expect(names(vert, 'struct')).toEqual(['Camera']);
    expect(names(vert, 'variable')).toContain('in_position');

    const frag = findGlslSymbols(fs.readFileSync(path.join(EXAMPLES, 'test.frag'), 'utf8'));
    expect(names(frag, 'function')).toEqual(['lambert', 'main']);
    expect(names(frag, 'variable')).toContain('LIGHT_DIRECTION');

    const comp = findGlslSymbols(fs.readFileSync(path.join(EXAMPLES, 'test.comp'), 'utf8'));
    expect(names(comp, 'function')).toEqual(['main']);
    expect(names(comp, 'struct')).toEqual(['Values', 'Params']);
  });
});
