export function formatGreeting(name: string): string {
  return `Hello, ${name}! Welcome to the VS Code extension monorepo.`
}

export function createLogMessage(level: 'info' | 'warn' | 'error', message: string): string {
  const timestamp = new Date().toISOString()
  return `[${timestamp}] [${level.toUpperCase()}] ${message}`
}
