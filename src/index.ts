import { runWizard } from './wizard.js'

runWizard().catch((err) => {
  console.error('  Ошибка:', err?.message ?? err)
  process.exit(1)
})
