# Security Policy

Thank you for helping keep **KeplerYard** secure. As an open-source project, maintaining a safe environment for users—especially when loading custom `.kyc` craft files, quicksaves, `.part.ron` definitions, and `.wesl` shaders—is important, though maintainer resources are limited.
Please review the policy below regarding supported versions and reporting vulnerabilities.

---

## Supported Versions

Security updates and vulnerability patches are exclusively provided for the latest Long-Term Support (LTS) release.

| Version          | Supported | Notes                                                 |
| ---------------- | --------- | ----------------------------------------------------- |
| >**1.0.0** (LTS) | Yes       | Active support for security vulnerabilities.          |
| < 1.0.0          | ❌ No     | Please upgrade to the latest LTS version immediately. |

> ⚠️ **Important:** To report a valid security issue, your environment **must** be actively running on the latest release. Bugs or vulnerabilities found on older, modified, or deprecated branches will not be triaged.

---

## Reporting a Vulnerability

If you discover a security vulnerability (such as arbitrary file write via save/blueprint paths, memory exhaustion via crafted `.kyc`/`.part.ron` payloads, or shader/asset parsing exploits), please do not disclose it publicly via public GitHub Issues or Pull Requests.

Please follow this process instead:

1. **Verify:** Ensure the vulnerability is reproducible on a clean build of version >**1.0.0**.
2. **Report:** Send a detailed report via email to **<joao.coutinho08@gmail.com>**.
3. **Include Details:**
   - A clear description of the vulnerability.
   - Steps to reproduce (or a Proof of Concept `.kyc` / `.part.ron` file or payload).
   - The potential impact of the exploit.

---

## Response Timeline

You will receive an acknowledgment of your report within **48-72 hours**.
If the vulnerability is verified as valid and within the scope of this project, a patch will be prepared and pushed to the main branch as a high priority.
