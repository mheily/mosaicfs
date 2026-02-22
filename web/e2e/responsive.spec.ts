import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('Responsive / Mobile', () => {
  test.use({ viewport: { width: 375, height: 667 } });

  test('sidebar is hidden and bottom tab bar is shown on mobile', async ({ page }) => {
    await login(page);

    // Desktop sidebar should be hidden (it uses hidden md:flex)
    const desktopSidebar = page.locator('aside');
    await expect(desktopSidebar).toBeHidden();

    // Mobile bottom nav bar should be visible
    const bottomNav = page.locator('nav.fixed');
    await expect(bottomNav).toBeVisible();
  });

  test('can navigate using bottom tab bar', async ({ page }) => {
    await login(page);

    // Tap "Search" in the bottom tabs
    await page.locator('nav.fixed').getByRole('link', { name: 'Search' }).click();
    await page.waitForURL('**/search');
    await expect(page.getByRole('heading', { name: 'Search' })).toBeVisible();

    // Tap "Labels"
    await page.locator('nav.fixed').getByRole('link', { name: 'Labels' }).click();
    await page.waitForURL('**/labels');
    await expect(page.getByRole('heading', { name: 'Labels' })).toBeVisible();

    // Tap "Settings"
    await page.locator('nav.fixed').getByRole('link', { name: 'Settings' }).click();
    await page.waitForURL('**/settings');
    await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
  });
});
