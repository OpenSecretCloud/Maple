/// <reference types="node" />

// This is an automated test for the signup process

import { config } from 'dotenv';
import { resolve } from 'path';
import { test, expect, Page } from '@playwright/test';

// Load environment variables from .env file
config({ path: resolve(__dirname, '../.env') });

const APP_URL = process.env.APP_URL;
const TESTER_EMAIL = process.env.TESTER_EMAIL;
const TESTER_PASSWORD = process.env.TESTER_PASSWORD;

test.beforeEach(async ({ page }) => {
  if (!APP_URL) {
    throw new Error('APP_URL environment variable is not set');
  }

  if (!TESTER_EMAIL) {
    throw new Error('TESTER_EMAIL environment variable is not set');
  }

  if (!TESTER_PASSWORD) {
    throw new Error('TESTER_PASSWORD environment variable is not set');
  }
});

test.skip('signup', async ({ page }) => {
  await page.goto(APP_URL);
  await page.getByRole('button', { name: 'Sign Up' }).click();
  await page.getByPlaceholder('email@example.com').click();
  
  // Generate a random email
  await page.getByPlaceholder('email@example.com').fill(TESTER_EMAIL);
  await page.getByLabel('Password').click();
  await page.getByLabel('Password').click();
  await page.getByLabel('Password').fill(TESTER_PASSWORD);
  await page.getByLabel('Invite Code').click();
  await page.getByLabel('Invite Code').fill('bearclaw24');
  await page.getByRole('button', { name: 'Create Account' }).click();

  // Check that the user logged in
  await expect(page.getByRole('heading', { name: 'History' })).toBeVisible();
});
