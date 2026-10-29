# The nushell the completion recording in tests/completion/ was made
# with: no banner, a neutral prompt, and of its shell integration only
# the OSC 133 marks.
$env.config.show_banner = false
$env.config.shell_integration.osc2 = false
$env.config.shell_integration.osc7 = false
$env.config.shell_integration.osc9_9 = false
$env.config.shell_integration.osc633 = false
$env.config.shell_integration.reset_application_mode = false
$env.config.shell_integration.osc133 = true
$env.PROMPT_COMMAND = {|| 'demo' }
$env.PROMPT_COMMAND_RIGHT = {|| '' }
$env.PROMPT_INDICATOR = {|| '> ' }
