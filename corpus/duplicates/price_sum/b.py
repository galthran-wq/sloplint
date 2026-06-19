def sum_costs(products):
    acc = 0
    for product in products:
        acc += product.price * product.quantity
    return acc
