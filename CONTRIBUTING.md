# Contributing to rdapx

First off — thank you for taking the time to contribute! 🎉  
Your help makes **rdapx** faster, smarter, and more useful for the whole community.

## 🧠 Philosophy
`rdapx` is built with clarity, performance, and simplicity in mind. Contributions should aim to:
- Improve real-world usability  
- Preserve clean, idiomatic Rust  
- Keep code fully async and Clippy-clean  
- Maintain zero warnings and minimal dependencies  

## 🧩 Getting Started
1. **Fork** the repository — https://github.com/evozeus/rdapx  
2. **Clone** your fork  
   `git clone https://github.com/<your-username>/rdapx`  
3. **Create a new branch**  
   `git checkout -b feature/your-feature-name`  
4. **Build and test locally:**

       cargo fmt
       cargo clippy -- -D warnings
       cargo test
       cargo run -- --format table get 1.1.1.1

5. When everything runs cleanly, **commit** your changes.

## 🧾 Commit Guidelines
Follow [Conventional Commits](https://www.conventionalcommits.org) for clear, descriptive history:

       feat: add caching for RDAP responses  
       fix: resolve panic on malformed JSON  
       refactor: simplify async lookup pipeline  
       docs: update README examples  
       test: add new bulk lookup tests  
       style: apply cargo fmt  

## 🧪 Testing
Ensure all checks pass before opening a PR:

       cargo fmt
       cargo clippy -- -D warnings
       cargo test

No warnings, no broken builds — that’s the **rdapx** way.

## 🧰 Pull Requests
- Keep PRs focused and scoped  
- Include a clear summary of what’s changed  
- Reference any related issues  
- Expect CI to check formatting, warnings, and tests  

Example PR title and body:

       feat: implement async bulk RDAP query mode  
       Adds stream-based bulk lookup with configurable concurrency, plus tests and docs.

## 📚 Documentation
All public functions must include Rustdoc (`///`) with:
- Purpose and behavior  
- Example usage if applicable  
- `# Errors` section for Result-returning functions  

## 🪪 Licensing
By submitting a PR, you agree that your contributions are licensed under the MIT License (see [LICENSE](LICENSE)).

## ❤️ Thank You
Every PR, issue, and suggestion helps make **rdapx** better.  
Thank you for being part of the project — your effort makes open tooling stronger for everyone.

— **Evozeus**
