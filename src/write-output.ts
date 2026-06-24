import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import type { ExtractedUnit } from './types.js'
import { formatUnit } from './format.js'

export function writeUnits(units: ExtractedUnit[], outDir: string, maxTokens = 90_000): string[] {
  mkdirSync(outDir, { recursive: true })
  const written: string[] = []
  for (const unit of units) {
    const { filename, parts } = formatUnit(unit, maxTokens)
    if (parts.length === 1) {
      const p = join(outDir, filename)
      writeFileSync(p, parts[0], 'utf8')
      written.push(p)
    } else {
      const stem = filename.replace(/\.md$/, '')
      parts.forEach((content, i) => {
        const p = join(outDir, `${stem}.part-${i + 1}.md`)
        writeFileSync(p, content, 'utf8')
        written.push(p)
      })
    }
  }
  return written
}
