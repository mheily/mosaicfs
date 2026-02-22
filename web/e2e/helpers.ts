import { type Page } from '@playwright/test';

const ACCESS_KEY_ID = 'admin';
const SECRET_KEY = process.env.MOSAIC_TEST_SECRET ?? 'admin';

/**
 * Log in to MosaicFS via the /login page.
 * Waits until the dashboard heading is visible before returning.
 */
export async function login(page: Page) {
  await page.goto('/login');
  await page.getByLabel('Access Key ID').fill(ACCESS_KEY_ID);
  await page.getByLabel('Secret Key').fill(SECRET_KEY);
  await page.getByRole('button', { name: 'Sign In' }).click();
  await page.waitForURL('**/');
  await page.getByRole('heading', { name: 'Dashboard' }).waitFor();
}
