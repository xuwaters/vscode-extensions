/** Leveled logging into the TS Server log (`fastElementUltra.logging`). */

export type LogLevel = 'off' | 'error' | 'warn' | 'debug' | 'verbose';

const LEVELS: Record<LogLevel, number> = {
  off: 0,
  error: 1,
  warn: 2,
  debug: 3,
  verbose: 4,
};

export class Logger {
  level: LogLevel = 'off';

  constructor(private sink: (message: string) => void) {}

  private write(level: LogLevel, message: string): void {
    if (LEVELS[this.level] >= LEVELS[level]) {
      this.sink(`[fast-element-ultra] ${level}: ${message}`);
    }
  }

  error(message: string): void {
    this.write('error', message);
  }

  warn(message: string): void {
    this.write('warn', message);
  }

  debug(message: string): void {
    this.write('debug', message);
  }

  verbose(message: string): void {
    this.write('verbose', message);
  }
}
