import { describe, it, expect, afterEach } from 'vitest'
import { mkdtempSync, rmSync, readdirSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { writeUnits } from '../src/write-output.js'
import type { ExtractedUnit } from '../src/types.js'

let dir = ''
afterEach(() => { if (dir) rmSync(dir, { recursive: true, force: true }) })

describe('writeUnits', () => {
  it('writes one file per single-part unit, parts suffix for split units', () => {
    dir = mkdtempSync(join(tmpdir(), 'tgsum-'))
    const units: ExtractedUnit[] = [
      { chatName: 'A', messages: [{ id: 1, type: 'message', date: '2026-06-18T09:00:00', from: 'X', from_id: 'u1', text: 'hi' }] },
      { chatName: 'B', messages: Array.from({ length: 30 }, (_, i) => ({ id: i, type: 'message' as const, date: '2026-06-18T09:00:00', from: 'X', from_id: 'u1', text: 'y'.repeat(40) })) },
    ]
    const written = writeUnits(units, dir, 60)
    const files = readdirSync(dir).sort()
    expect(files).toContain('A.md')
    expect(files.some(f => f === 'B.part-1.md')).toBe(true)
    expect(written.length).toBe(files.length)
    expect(readFileSync(join(dir, 'A.md'), 'utf8')).toContain('# Чат: A')
  })

  it('de-duplicates colliding filenames instead of overwriting', () => {
    dir = mkdtempSync(join(tmpdir(), 'tgsum-'))
    const units: ExtractedUnit[] = [
      { chatName: 'Dup', messages: [{ id: 1, type: 'message', date: '2026-06-18T09:00:00', from: 'X', from_id: 'u1', text: 'first' }] },
      { chatName: 'Dup', messages: [{ id: 2, type: 'message', date: '2026-06-18T09:01:00', from: 'Y', from_id: 'u2', text: 'second' }] },
    ]
    const written = writeUnits(units, dir, 100_000)
    const files = readdirSync(dir).sort()
    expect(files).toContain('Dup.md')
    expect(files).toContain('Dup (2).md')
    expect(new Set(written).size).toBe(2)
    expect(written.length).toBe(2)
    // Both units' content is preserved (no overwrite).
    expect(readFileSync(join(dir, 'Dup.md'), 'utf8')).toContain('first')
    expect(readFileSync(join(dir, 'Dup (2).md'), 'utf8')).toContain('second')
  })

  it('clears stale part files when a re-run produces fewer parts', () => {
    dir = mkdtempSync(join(tmpdir(), 'tgsum-'))
    const big = (n: number): ExtractedUnit => ({
      chatName: 'C',
      messages: Array.from({ length: n }, (_, i) => ({
        id: i, type: 'message' as const, date: '2026-06-18T09:00:00', from: 'X', from_id: 'u1', text: 'z'.repeat(40),
      })),
    })
    // First run: many messages → several parts.
    writeUnits([big(40)], dir, 60)
    const firstParts = readdirSync(dir).filter(f => /^C\.part-\d+\.md$/.test(f)).length
    expect(firstParts).toBeGreaterThan(2)
    // Second run into the same dir: fewer messages → fewer parts; no stale leftovers.
    writeUnits([big(6)], dir, 60)
    const after = readdirSync(dir).filter(f => /^C\.part-\d+\.md$/.test(f)).length
    expect(after).toBeLessThan(firstParts)
    // No part number beyond the new count should survive.
    const maxPart = Math.max(...readdirSync(dir).map(f => Number(/^C\.part-(\d+)\.md$/.exec(f)?.[1] ?? 0)))
    expect(maxPart).toBe(after)
  })
})
