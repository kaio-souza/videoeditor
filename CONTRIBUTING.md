# Como contribuir

Obrigado por considerar contribuir com o Editor de Vídeo em Rust. Este documento explica como preparar o ambiente, o que é esperado em cada mudança e como enviar um pull request.

## Antes de começar

- Leia o [README.md](README.md) para entender o escopo do projeto.
- O [CLAUDE.md](CLAUDE.md) contém a especificação técnica completa: arquitetura, modelo de dados, pipeline de preview/export e o roadmap por fases. Mudanças estruturais devem respeitar essa arquitetura (em especial o mapeamento de tempo *Timeline → Clip → Source*, §78 e §117).
- Para bugs ou ideias grandes, abra uma issue antes de escrever código, para alinhar a abordagem.

## Ambiente de desenvolvimento

Requisitos: Rust stable com suporte a edition 2024.

```bash
cargo build
cargo run
```

No Linux, os headers do ALSA necessários para compilar o áudio já estão vendorizados em `vendor/alsa/`. O `ffmpeg`/`ffprobe` estático é baixado automaticamente na primeira execução (via `ffmpeg-sidecar`). Não remova essa chamada sem substituí-la por outro mecanismo de obtenção do FFmpeg.

## Critério de conclusão

Toda mudança precisa deixar estes quatro comandos limpos antes de ser enviada:

```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo build
```

Além disso:

- Crie testes para lógica nova (em `src/` ou em `tests/`), principalmente para timeline, reverse, fade, mixer e serialização.
- Não quebre funcionalidades existentes.
- Não adicione dependências sem necessidade. Ao adicionar uma crate, verifique a versão atual e a licença dela.

## Convenções

### Textos da interface

Todo texto visível ao usuário passa por `crate::i18n::tr("chave")`. As traduções ficam em `locales/app.yml`, com as 10 línguas (en, pt-BR, es, zh-CN, ja, hi, id, fr, it, de) para cada chave. Não coloque texto de interface diretamente no código.

Rótulos de trilha (V1/V2/A1/A2), timecodes e dicas de atalho de teclado ficam sem tradução, por convenção de editores não-lineares.

### Mídia e arquivos originais

O editor é não destrutivo. Código novo não deve modificar arquivos de mídia originais nem sobrescrever a origem durante o export.

### Mensagens de commit

Use o formato de commits convencionais, em minúsculas, com um tipo e um escopo curto quando fizer sentido:

```text
feat: add audio fade-out
fix: keep reverse mapping after split
docs: update export section in README
```

Um commit deve conter uma mudança coerente. Prefira vários commits pequenos a um commit gigante.

## Enviando um pull request

1. Faça um fork e crie uma branch a partir de `master`.
2. Faça as mudanças seguindo as convenções acima.
3. Rode `cargo fmt`, `cargo clippy --all-targets`, `cargo test` e `cargo build`.
4. Abra o PR descrevendo o que mudou, por quê e como foi testado. Se a mudança afeta a interface, inclua capturas de tela.

O CI (`.github/workflows/rust.yml`) roda `cargo build` e `cargo test` em cada push e PR para `master`.

## Licença das contribuições

Ao enviar uma contribuição, você concorda que ela será distribuída sob a [licença MIT](LICENSE) do projeto.
