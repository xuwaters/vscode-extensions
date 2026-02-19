import * as vscode from 'vscode'
import { formatGreeting, createLogMessage } from '@ext/common'

let outputChannel: vscode.OutputChannel

export function activate(context: vscode.ExtensionContext): void {
  outputChannel = vscode.window.createOutputChannel('Sample Extension')

  outputChannel.appendLine(
    createLogMessage('info', 'Sample Extension is now active!')
  )

  const helloWorldCommand = vscode.commands.registerCommand(
    'sample-ext.helloWorld',
    () => {
      const greeting = formatGreeting('World')
      vscode.window.showInformationMessage(greeting)
      outputChannel.appendLine(
        createLogMessage('info', `Command executed: ${greeting}`)
      )
    }
  )

  context.subscriptions.push(helloWorldCommand)
  context.subscriptions.push(outputChannel)
}

export function deactivate(): void {}
