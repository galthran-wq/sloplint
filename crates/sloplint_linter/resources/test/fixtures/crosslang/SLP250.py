"""Fixture for SLP250 — cross-language pollution."""

# --- Violations: unambiguously foreign idioms ---

a = obj.toString()  # JS/Java
c = s.charAt(0)  # JS/Java
upper = s.toUpperCase()  # JS/Java
idx = items.indexOf(x)  # JS/Java
items.forEach(handler)  # JS
n = arr.length  # JS/Java attribute
console.log(value)  # JS
array_push(bucket, item)  # PHP
println("hello")  # Java/Kotlin/Go


# --- Non-violations: legitimate Python that merely looks foreign ---

clean = re.sub(pattern, repl, text)  # stdlib
click.echo("ok")  # click
stack.push(item)  # a real stack with a push() method
has = frame.contains("x")  # pandas .contains
total = queue.size()  # a real .size() method
got = mapping.get("key")  # dict.get
xs.append(1)  # list.append
joined = ",".join(parts)  # str.join
found = text.find("y")  # str.find
