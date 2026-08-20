defmodule Acceptance.Script do
  def run(), do: Acceptance.Worker.run(:script)
end
