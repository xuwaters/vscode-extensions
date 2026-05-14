import * as vscode from 'vscode';
import { REF_SCHEME, parseRefUriInfo, type RefSide } from './refContentProvider';

class RefDecorationProvider implements vscode.FileDecorationProvider {
  provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
    if (uri.scheme !== REF_SCHEME) return undefined;
    const info = parseRefUriInfo(uri);
    if (!info) return undefined;
    const deco = new vscode.FileDecoration(
      badgeForSide(info.side),
      info.label,
      new vscode.ThemeColor('gitDecoration.untrackedResourceForeground'),
    );
    deco.propagate = false;
    return deco;
  }
}

// A comparison only ever has two sides, so use a stable per-side badge rather
// than deriving it from the ref name. Hover still shows the full ref label.
function badgeForSide(side: RefSide): string {
  return side === 'working' ? 'W' : 'C';
}

export function registerRefDecorationProvider(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.window.registerFileDecorationProvider(new RefDecorationProvider()),
  );
}
