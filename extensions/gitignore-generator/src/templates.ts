export interface Template {
  id: string;
  label: string;
  description: string;
  content: string;
}

const DEFAULT = `
# macOS
.DS_Store

# Rust
target
**/*.rs.bk
Cargo.lock.bak

# Node
node_modules
dist
.vite
.next
.astro
.cache
.turbo
.yarn
.pnpm-store

# Logs
*.log
npm-debug.log*
yarn-debug.log*
yarn-error.log*
pnpm-debug.log*

# Env
.env
.env.*.local
.env.local
!.env.example

# Editor / misc
*.local
*.zip
temp
.claude
.vscode

# sqlx — keep \`.sqlx/\` committed per RFC 0012 (offline query data)
# but ignore the tmp cache directory
.sqlx-cache
*.db

# Python
__pycache__
`;

const NODE = `# Dependencies
node_modules/
jspm_packages/

# Logs
logs
*.log
npm-debug.log*
yarn-debug.log*
yarn-error.log*
pnpm-debug.log*
lerna-debug.log*

# Runtime data
pids
*.pid
*.seed
*.pid.lock

# Coverage / test output
coverage/
*.lcov
.nyc_output

# Build output
dist/
build/
out/
.next/
.nuxt/
.svelte-kit/

# Caches
.cache/
.parcel-cache/
.turbo/
.eslintcache
.stylelintcache

# Environment
.env
.env.*
!.env.example

# Optional npm cache directory
.npm

# Yarn
.yarn/cache
.yarn/unplugged
.yarn/build-state.yml
.yarn/install-state.gz
.pnp.*
`;

const PYTHON = `# Byte-compiled / optimized / DLL files
__pycache__/
*.py[cod]
*$py.class

# C extensions
*.so

# Distribution / packaging
.Python
build/
develop-eggs/
dist/
downloads/
eggs/
.eggs/
lib/
lib64/
parts/
sdist/
var/
wheels/
*.egg-info/
.installed.cfg
*.egg
MANIFEST

# PyInstaller
*.manifest
*.spec

# Unit test / coverage
htmlcov/
.tox/
.nox/
.coverage
.coverage.*
.cache
nosetests.xml
coverage.xml
*.cover
.pytest_cache/
.hypothesis/

# Environments
.venv
venv/
env/
ENV/
.env

# Tooling
.mypy_cache/
.ruff_cache/
.pyre/
.pytype/

# Jupyter
.ipynb_checkpoints
`;

const RUST = `# Cargo build artifacts
/target
**/target

# Cargo lock for libraries (delete this line for binaries)
# Cargo.lock

# Backup files
**/*.rs.bk

# mdBook
/book
`;

const GO = `# Binaries
*.exe
*.exe~
*.dll
*.so
*.dylib

# Test and coverage
*.test
*.out
coverage.txt

# Go workspace file
go.work
go.work.sum

# Vendor
vendor/
`;

const JAVA = `# Compiled class files
*.class

# Log files
*.log

# Package files
*.jar
*.war
*.nar
*.ear
*.zip
*.tar.gz
*.rar

# Virtual machine crash logs
hs_err_pid*
replay_pid*

# Build tools
target/
build/
out/
.gradle/
!gradle-wrapper.jar

# Maven
pom.xml.tag
pom.xml.releaseBackup
pom.xml.versionsBackup
pom.xml.next
release.properties
dependency-reduced-pom.xml
`;

const MACOS = `.DS_Store
.AppleDouble
.LSOverride
Icon
._*

# Thumbnails
.Spotlight-V100
.Trashes

# Files that might appear in the root of a volume
.DocumentRevisions-V100
.fseventsd
.TemporaryItems
.VolumeIcon.icns
.com.apple.timemachine.donotpresent
`;

const WINDOWS = `# Windows thumbnail cache files
Thumbs.db
Thumbs.db:encryptable
ehthumbs.db
ehthumbs_vista.db

# Dump file
*.stackdump

# Folder config file
[Dd]esktop.ini

# Recycle Bin used on file shares
$RECYCLE.BIN/

# Windows Installer files
*.cab
*.msi
*.msix
*.msm
*.msp

# Windows shortcuts
*.lnk
`;

const LINUX = `*~

# Temporary files which can be created if a process still has a handle open of a deleted file
.fuse_hidden*

# KDE directory preferences
.directory

# Linux trash folder which might appear on any partition or disk
.Trash-*

# .nfs files are created when an open file is removed but is still being accessed
.nfs*
`;

const VSCODE = `.vscode/*
!.vscode/settings.json
!.vscode/tasks.json
!.vscode/launch.json
!.vscode/extensions.json
!.vscode/*.code-snippets

# Local History for Visual Studio Code
.history/

# Built Visual Studio Code Extensions
*.vsix
`;

const JETBRAINS = `.idea/
*.iml
*.iws
*.ipr
out/

# CMake
cmake-build-*/

# File-based project format
*.iws

# Crashlytics plugin (for Android Studio and IntelliJ)
com_crashlytics_export_strings.xml
crashlytics.properties
crashlytics-build.properties
fabric.properties
`;

export const TEMPLATES: Template[] = [
  { id: 'default', label: 'Default', description: 'Default', content: DEFAULT },
  { id: 'node', label: 'Node', description: 'Node.js, npm, pnpm, yarn', content: NODE },
  { id: 'python', label: 'Python', description: 'Python, pip, venv, pytest', content: PYTHON },
  { id: 'rust', label: 'Rust', description: 'Cargo build artifacts', content: RUST },
  { id: 'go', label: 'Go', description: 'Go build output and tooling', content: GO },
  { id: 'java', label: 'Java', description: 'Java, Maven, Gradle', content: JAVA },
  { id: 'macos', label: 'macOS', description: 'macOS Finder metadata', content: MACOS },
  { id: 'windows', label: 'Windows', description: 'Windows system files', content: WINDOWS },
  { id: 'linux', label: 'Linux', description: 'Linux temporary files', content: LINUX },
  { id: 'vscode', label: 'VS Code', description: '.vscode workspace files', content: VSCODE },
  { id: 'jetbrains', label: 'JetBrains', description: 'IntelliJ / PyCharm / WebStorm', content: JETBRAINS },
];

export function getTemplate(id: string): Template | undefined {
  return TEMPLATES.find(t => t.id === id);
}
