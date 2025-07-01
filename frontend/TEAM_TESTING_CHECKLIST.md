# Team Management Testing Checklist

## Initial Setup Flow
1. [ ] **Purchase Team Plan**
   - Sign in as a regular user
   - Go to /pricing
   - Verify team plans are visible (no "Contact Us" button)
   - Purchase a team plan through Stripe

2. [ ] **Team Creation**
   - After purchase, click "Manage Team" in AccountMenu
   - Verify "Setup Required" badge appears
   - Verify TeamSetupDialog opens automatically
   - Enter a team name and create team
   - Verify dialog closes and dashboard appears

## Team Dashboard Testing
3. [ ] **Dashboard UI**
   - Verify team name displays correctly
   - Check "Admin" badge appears for creator
   - Verify seat usage shows correctly (1/X seats used)
   - Check member count shows 1
   - Verify "Invite Members" button is enabled

4. [ ] **Invite Members**
   - Click "Invite Members" button
   - Test single email entry
   - Test multiple emails (comma-separated)
   - Test multiple emails (line-separated)
   - Verify email validation works
   - Check seat limit validation
   - Send invites and verify success message

5. [ ] **Member List**
   - Verify your own account shows with "You" badge
   - Check pending invites appear below members
   - Verify expiration time shows for invites
   - Test revoke invite functionality (X button)

## Edge Cases to Test
6. [ ] **Seat Limits**
   - Try inviting more members than available seats
   - Verify error message appears
   - Check if "Invite Members" button disables when at capacity

7. [ ] **Refresh Behavior**
   - Refresh page and verify team status persists
   - Check that team dashboard loads correctly
   - Verify member list refreshes after actions

## What's NOT Ready Yet
- Invite acceptance (recipients can't join yet - Task 11)
- Removing members (backend might need testing)
- Leave team functionality (backend might need testing)
- Error toasts for better feedback (currently console.error)

## Console Monitoring
Watch browser console for:
- API errors (401, 403, 404, etc.)
- Failed requests
- Any JavaScript errors

## Quick Test Script
```bash
# In one terminal
cd frontend
bun run dev

# Check for any compilation errors
# Open http://localhost:5173
```