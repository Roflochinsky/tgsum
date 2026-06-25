import { styleText } from 'node:util'
import { AutocompletePrompt, settings } from '@clack/core'
import {
  limitOptions,
  symbol,
  S_BAR,
  S_BAR_END,
  S_CHECKBOX_SELECTED,
  S_CHECKBOX_INACTIVE,
} from '@clack/prompts'

// A searchable multi-select identical in behaviour to @clack/prompts'
// autocompleteMultiselect, but with an explicit GREEN highlight so the
// currently-focused row and the selected rows are obvious even in terminals
// that don't render `dim` (clack's default only un-dims the active row).

type Opt = { value: string; label?: string; hint?: string; disabled?: boolean }

// Exported for testing. `active` = the focused (cursor) row.
export function styleItem(opt: Opt, active: boolean, selected: string[], focused?: string): string {
  const isSel = selected.includes(opt.value)
  const label = opt.label ?? String(opt.value ?? '')
  const hint = opt.hint && focused !== undefined && opt.value === focused
    ? styleText('dim', ` (${opt.hint})`)
    : ''
  const box = isSel ? styleText('green', S_CHECKBOX_SELECTED) : styleText('dim', S_CHECKBOX_INACTIVE)
  if (opt.disabled) {
    return `  ${styleText('gray', S_CHECKBOX_INACTIVE)} ${styleText(['strikethrough', 'gray'], label)}`
  }
  if (active) return `${styleText('green', '›')} ${box} ${styleText('green', label)}${hint}`
  if (isSel) return `  ${box} ${styleText('green', label)}${hint}`
  return `  ${box} ${styleText('dim', label)}`
}

export interface AutoMultiOpts {
  message: string
  options: Opt[]
  placeholder?: string
  maxItems?: number
}

export function colorAutocompleteMultiselect(opts: AutoMultiOpts): Promise<string[] | symbol> {
  const filter = (input: string, option: Opt) =>
    (option.label ?? String(option.value ?? '')).toLowerCase().includes(input.toLowerCase())

  // ponytail: faithful copy of clack's autocompleteMultiselect render() with one
  // change — styleItem paints the active/selected rows green. `this` is the prompt
  // instance; typed loosely to avoid clack's deep render generics.
  const prompt = new AutocompletePrompt({
    options: opts.options,
    multiple: true,
    placeholder: opts.placeholder,
    filter,
    render(this: any) {
      const withGuide = settings.withGuide
      const title = `${withGuide ? styleText('gray', S_BAR) + '\n' : ''}${symbol(this.state)}  ${opts.message}\n`
      const ph = opts.placeholder
      const showPh = this.userInput === '' && ph !== undefined
      const value = this.isNavigating || showPh
        ? styleText('dim', showPh ? ph! : this.userInput)
        : this.userInputWithCursor
      const matchInfo = this.filteredOptions.length !== this.options.length
        ? styleText('dim', ` (${this.filteredOptions.length} match${this.filteredOptions.length === 1 ? '' : 'es'})`)
        : ''

      if (this.state === 'submit') {
        return `${title}${withGuide ? styleText('gray', S_BAR) + '  ' : ''}${styleText('dim', `${this.selectedValues.length} items selected`)}`
      }
      if (this.state === 'cancel') {
        return `${title}${withGuide ? styleText('gray', S_BAR) + '  ' : ''}${styleText(['strikethrough', 'dim'], this.userInput)}`
      }

      const color = this.state === 'error' ? 'yellow' : 'cyan'
      const bar = withGuide ? `${styleText(color, S_BAR)}  ` : ''
      const barEnd = withGuide ? styleText(color, S_BAR_END) : ''
      const footer = [
        `${styleText('dim', '↑/↓')} to navigate`,
        `${styleText('dim', this.isNavigating ? 'Space/Tab:' : 'Tab:')} select`,
        `${styleText('dim', 'Enter:')} confirm`,
        `${styleText('dim', 'Type:')} to search`,
      ]
      const noMatch = this.filteredOptions.length === 0 && this.userInput
        ? [`${bar}${styleText('yellow', 'No matches found')}`] : []
      const errLines = this.state === 'error' ? [`${bar}${styleText('yellow', this.error)}`] : []
      const head = [
        ...`${title}${withGuide ? styleText(color, S_BAR) : ''}`.split('\n'),
        `${bar}${styleText('dim', 'Search:')} ${value}${matchInfo}`,
        ...noMatch,
        ...errLines,
      ]
      const foot = [`${bar}${footer.join(' • ')}`, barEnd]
      const body = limitOptions({
        cursor: this.cursor,
        options: this.filteredOptions,
        style: (o: Opt, active: boolean) => styleItem(o, active, this.selectedValues, this.focusedValue),
        maxItems: opts.maxItems,
        rowPadding: head.length + foot.length,
      } as any)
      return [...head, ...body.map((f: string) => `${bar}${f}`), ...foot].join('\n')
    },
  } as any)

  return prompt.prompt() as Promise<string[] | symbol>
}
