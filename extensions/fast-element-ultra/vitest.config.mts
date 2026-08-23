import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['test/**/*.test.ts'],
    environment: 'node',
    // The corpus tests build real TypeScript programs; give them room.
    testTimeout: 60_000,
    hookTimeout: 60_000,
  },
});
