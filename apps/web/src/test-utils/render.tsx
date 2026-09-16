import type { ReactNode } from 'react';
import { render as testingLibraryRender } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';

/**
 * Render a component inside a `MantineProvider`, which Mantine components
 * require in order to resolve theme values.
 */
export function render(ui: ReactNode) {
  return testingLibraryRender(<>{ui}</>, {
    wrapper: ({ children }) => <MantineProvider>{children}</MantineProvider>,
  });
}

export * from '@testing-library/react';
